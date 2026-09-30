//! HTTP client for ModularGrid's internal (undocumented) endpoints.
//!
//! The site is a CakePHP app. Most interactive actions are jQuery AJAX calls to
//! `/<format>/<controller>/<action>.json` returning `{"response": {"success":
//! bool, "result"|"msg": ...}}`; the rest are classic HTML form posts.

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::{Client as Http, Response};
use reqwest::cookie::{CookieStore, Jar};
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use url::Url;

use crate::session::{Cookie, Session};

pub const ORIGIN: &str = "https://modulargrid.net";
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; modulargrid-cli/",
    env!("CARGO_PKG_VERSION"),
    ")"
);

pub struct Client {
    http: Http,
    jar: Arc<Jar>,
    /// Format prefix, e.g. "e" for Eurorack.
    format: String,
}

#[derive(Debug, Serialize)]
pub struct Module {
    pub id: u64,
    pub name: String,
    pub vendor: String,
    pub slug: String,
    pub hp: Option<u32>,
    pub description: String,
    pub price: Option<String>,
}

/// Filters accepted by `modules/find` (mirrors the website's search form).
#[derive(Debug, Default)]
pub struct SearchFilters {
    pub name: String,
    pub vendor: Option<u64>,
    pub function: Option<u64>,
    pub secondary_function: Option<u64>,
    pub exclude_secondary: bool,
    /// "" (all), "f" (full size), "h" (1U tiles), "hij" (1U Intellijel), "hpl" (1U Pulp Logic).
    pub height: Option<String>,
    pub hp: Option<u32>,
    pub hp_exact: bool,
    pub max_depth: Option<u32>,
    /// "a" (assembled) or "d" (DIY).
    pub build: Option<String>,
    /// concept | available | discontinued | unassigned
    pub lifecycle: Option<String>,
    pub mine: bool,
    pub marketplace: Option<u64>,
    pub modeled: bool,
    pub others: bool,
    pub passive: bool,
    /// newest | popular | alphabetic | price | manuf | hp | power | depth | tag
    pub order: Option<String>,
    pub desc: bool,
}

/// A named option from one of the search form's `<select>` lists.
#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct RackSummary {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct RackModule {
    /// Id of this module *instance* in the rack (ModulesRack.id).
    pub instance_id: u64,
    pub module_id: u64,
    pub name: String,
    pub vendor: String,
    pub description: String,
    pub hp: u32,
    pub row: u32,
    pub col: u32,
    /// False for modules parked outside the rack area.
    pub in_bounds: bool,
    /// A 1U tile rather than a full-height (3U) module.
    pub is_1u: bool,
    pub current_plus_12v: u64,
    pub current_minus_12v: u64,
    pub current_5v: u64,
}

#[derive(Debug, Serialize)]
pub struct RackTotals {
    pub current_plus_12v: u64,
    pub current_minus_12v: u64,
    pub current_5v: u64,
    pub modules: u64,
    pub max_depth_mm: Option<u64>,
    pub price_eur: Option<String>,
    pub price_usd: Option<String>,
    /// Modules missing power/price data, as reported by the site.
    pub incomplete: Option<String>,
}

impl RackModule {
    pub fn width(&self) -> u32 {
        self.hp.max(1)
    }
}

#[derive(Debug, Serialize)]
pub struct Rack {
    pub id: u64,
    pub name: String,
    pub rows: u32,
    pub hp: u32,
    /// Rows (1-based) that are 1U rows.
    pub rows_1u: Vec<u32>,
    pub private: bool,
    pub owner: String,
    pub modules: Vec<RackModule>,
}

impl Rack {
    /// The server accepts any position, so check bounds and overlaps here
    /// (the website's planner does the same client-side).
    pub fn check_fit(&self, module: &RackModule, row: u32, col: u32) -> Result<()> {
        if row < 1 || row > self.rows {
            bail!("row {row} is outside the rack (rows 1-{})", self.rows);
        }
        let row_is_1u = self.rows_1u.contains(&row);
        if module.is_1u != row_is_1u {
            let (what, kind) = if module.is_1u {
                ("a 1U tile", "3U")
            } else {
                ("a 3U module", "1U")
            };
            bail!(
                "{} {} is {what}, but row {row} is a {kind} row",
                module.vendor,
                module.name
            );
        }
        let (width, ignore_instance) = (module.width(), Some(module.instance_id));
        let end = col + width - 1;
        if col < 1 || end > self.hp {
            bail!(
                "a {width} HP module at col {col} would span {}, outside the rack (HP 1-{})",
                hp_span(col, end),
                self.hp
            );
        }
        if let Some(other) = self.modules.iter().find(|m| {
            Some(m.instance_id) != ignore_instance
                && m.in_bounds
                && m.row == row
                && m.col <= end
                && col < m.col + m.width()
        }) {
            bail!(
                "row {row}, {} overlaps {} {} (instance {}, {})",
                hp_span(col, end),
                other.vendor,
                other.name,
                other.instance_id,
                hp_span(other.col, other.col + other.width() - 1)
            );
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct WhoAmI {
    pub username: String,
    pub user_id: Option<String>,
}

pub struct NewRack<'a> {
    pub name: &'a str,
    pub hp: u32,
    pub rows: u32,
    pub rows_1u: &'a [u32],
    pub private: bool,
    pub format: &'a str,
    pub url: &'a str,
    pub theme: Option<u32>,
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("valid selector")
}

fn text(e: ElementRef) -> String {
    e.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Client {
    pub fn new(session: Option<&Session>, format: &str) -> Result<Self> {
        let jar = Arc::new(Jar::default());
        let origin: Url = ORIGIN.parse()?;
        if let Some(s) = session {
            for c in &s.cookies {
                jar.add_cookie_str(
                    &format!("{}={}; Domain=modulargrid.net; Path=/", c.name, c.value),
                    &origin,
                );
            }
        }
        let http = Http::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(jar.clone())
            .build()?;
        Ok(Self {
            http,
            jar,
            format: format.to_string(),
        })
    }

    /// Current cookies held by the client (the server may refresh them).
    pub fn cookies(&self) -> Vec<Cookie> {
        let origin: Url = ORIGIN.parse().unwrap();
        let Some(h) = self.jar.cookies(&origin) else {
            return vec![];
        };
        h.to_str()
            .unwrap_or_default()
            .split("; ")
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| Cookie {
                name: k.to_string(),
                value: v.to_string(),
            })
            .collect()
    }

    fn url(&self, path: &str) -> String {
        format!("{ORIGIN}/{}/{}", self.format, path.trim_start_matches('/'))
    }

    fn get_html(&self, path: &str, query: &[(&str, String)]) -> Result<(Url, String)> {
        let r = self
            .http
            .get(self.url(path))
            .query(query)
            .send()?
            .error_for_status()?;
        let final_url = r.url().clone();
        Ok((final_url, r.text()?))
    }

    /// Call one of the `*.json` AJAX endpoints and unwrap the envelope.
    fn ajax(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let r: Response = self
            .http
            .get(self.url(path))
            .query(query)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json")
            .send()?;
        if r.status() == reqwest::StatusCode::FORBIDDEN {
            bail!("not logged in (or session expired) — run `modulargrid login`");
        }
        let r = r.error_for_status()?;
        let body = r.text()?;
        let v: Value = serde_json::from_str(&body).with_context(|| {
            format!(
                "unexpected non-JSON response from {path}: {}",
                truncate(&body, 200)
            )
        })?;
        let resp = &v["response"];
        if resp["success"].as_bool() == Some(true) {
            Ok(resp["result"].clone())
        } else {
            let msg = resp["msg"].as_str().unwrap_or("request failed");
            Err(anyhow!("{msg}"))
        }
    }

    fn require_login(&self, html: &str, final_url: &Url) -> Result<()> {
        if final_url.path().ends_with("/users/login") || is_anonymous(html) {
            bail!("not logged in (or session expired) — run `modulargrid login`");
        }
        Ok(())
    }

    // ----- account -------------------------------------------------------

    pub fn whoami(&self) -> Result<Option<WhoAmI>> {
        // Anonymous visitors get a 404 here (or a redirect to the login page).
        let r = self.http.get(self.url("users/view")).send()?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let r = r.error_for_status()?;
        let u = r.url().clone();
        let html = r.text()?;
        if u.path().ends_with("/users/login") || is_anonymous(&html) {
            return Ok(None);
        }
        let doc = Html::parse_document(&html);
        let username = doc
            .select(&sel("title"))
            .next()
            .map(text)
            .and_then(|t| {
                t.strip_prefix("User ")
                    .and_then(|t| t.strip_suffix(" on ModularGrid"))
                    .map(str::to_string)
            })
            .or_else(|| doc.select(&sel("h1")).next().map(text))
            .context("could not determine username from profile page")?;
        let user_id = doc.select(&sel("a[href]")).find_map(|a| {
            let h = a.value().attr("href")?;
            let rest = h.split("/racks/command_center/").nth(1)?;
            let id: String = rest.chars().take_while(char::is_ascii_digit).collect();
            (!id.is_empty()).then_some(id)
        });
        Ok(Some(WhoAmI { username, user_id }))
    }

    pub fn logout(&self) -> Result<()> {
        self.http.get(self.url("users/logout")).send()?;
        Ok(())
    }

    // ----- modules -------------------------------------------------------

    /// Search modules with the same filters as the website's module browser.
    ///
    /// The site hides modules filed under the "Other/unknown" vendor unless
    /// `SearchShowothers=1`; that's always on for `mine` so collection
    /// listings are complete.
    pub fn search(&self, f: &SearchFilters, limit: usize) -> Result<(usize, Vec<Module>)> {
        let opt = |v: &Option<String>| v.clone().unwrap_or_default();
        let flag = |b: bool| if b { "1" } else { "0" }.to_string();
        let mut params: Vec<(&str, String)> = vec![
            ("SearchName", f.name.clone()),
            (
                "SearchVendor",
                f.vendor.map(|v| v.to_string()).unwrap_or_default(),
            ),
            (
                "SearchFunction",
                f.function.map(|v| v.to_string()).unwrap_or_default(),
            ),
            (
                "SearchSecondaryfunction",
                f.secondary_function
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            ),
            ("SearchSecondaryfunctionexcl", flag(f.exclude_secondary)),
            ("SearchHeight", opt(&f.height)),
            ("SearchTe", f.hp.map(|v| v.to_string()).unwrap_or_default()),
            (
                "SearchTemethod",
                if f.hp_exact { "exact" } else { "max" }.to_string(),
            ),
            (
                "SearchMaxdepth",
                f.max_depth.map(|v| v.to_string()).unwrap_or_default(),
            ),
            ("SearchBuildtype", opt(&f.build)),
            ("SearchLifecycle", opt(&f.lifecycle)),
            ("SearchSet", if f.mine { "my" } else { "all" }.to_string()),
            (
                "SearchMarketplace",
                f.marketplace.map(|v| v.to_string()).unwrap_or_default(),
            ),
            ("SearchIsmodeled", flag(f.modeled)),
            ("SearchShowothers", flag(f.others || f.mine)),
            ("SearchOnlypassive", flag(f.passive)),
            ("order", opt(&f.order)),
            ("direction", if f.desc { "desc" } else { "asc" }.to_string()),
        ];
        let mine = f.mine;
        let mut path = "modules/find".to_string();
        let mut out: Vec<Module> = Vec::new();
        let mut total = 0;
        let mut seen = std::collections::HashSet::new();
        loop {
            let (u, html) = self.get_html(&path, &params)?;
            if mine {
                self.require_login(&html, &u)?;
            }
            let doc = Html::parse_fragment(&html);
            if let Some(c) = doc.select(&sel("#search-count")).next() {
                total = c
                    .value()
                    .attr("data-search-count")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            }
            for m in doc.select(&sel(".box-module")).filter_map(parse_module_box) {
                // Pages can overlap slightly; de-duplicate.
                if seen.insert(m.id) {
                    out.push(m);
                }
            }
            if out.len() >= limit {
                out.truncate(limit);
                break;
            }
            let Some(next) = doc
                .select(&sel("#lnk-next-results"))
                .next()
                .and_then(|a| a.value().attr("href"))
            else {
                break;
            };
            // The next link already carries the full query string.
            let prefix = format!("/{}/", self.format);
            path = next.strip_prefix(&prefix).unwrap_or(next).to_string();
            params.clear();
        }
        if mine && out.len() > total {
            total = out.len();
        }
        Ok((total, out))
    }

    /// Options of a `<select>` on the module browser's search form
    /// (`SearchVendor`, `SearchFunction`, `SearchMarketplace`).
    pub fn choices(&self, select_id: &str) -> Result<Vec<Choice>> {
        let (_, html) = self.get_html("modules/browser", &[])?;
        let doc = Html::parse_document(&html);
        let select = doc
            .select(&sel(&format!("select#{select_id}")))
            .next()
            .with_context(|| format!("search form has no {select_id} list"))?;
        Ok(select
            .select(&sel("option"))
            .filter_map(|o| {
                let id = o.value().attr("value")?.parse().ok()?;
                Some(Choice { id, name: text(o) })
            })
            .collect())
    }

    /// Resolve a name (or id) against one of the search form's lists: exact
    /// match first, then a unique case-insensitive substring match.
    pub fn resolve_choice(&self, select_id: &str, what: &str, input: &str) -> Result<u64> {
        if let Ok(id) = input.parse() {
            return Ok(id);
        }
        let all = self.choices(select_id)?;
        let lc = input.to_lowercase();
        if let Some(c) = all.iter().find(|c| c.name.to_lowercase() == lc) {
            return Ok(c.id);
        }
        let hits: Vec<_> = all
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&lc))
            .collect();
        match hits.as_slice() {
            [one] => Ok(one.id),
            [] => bail!("no {what} matching '{input}'"),
            many => bail!(
                "'{input}' matches several {what}s: {}",
                many.iter()
                    .take(12)
                    .map(|c| format!("{} ({})", c.name, c.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    /// Resolve a module reference (numeric id, slug, or URL) to its id.
    pub fn resolve_module(&self, r: &str) -> Result<u64> {
        if let Ok(id) = r.parse() {
            return Ok(id);
        }
        let slug = r
            .trim_end_matches('/')
            .rsplit_once(&format!("/{}/", self.format))
            .map(|(_, s)| s)
            .unwrap_or(r);
        let resp = self.http.get(self.url(slug)).send()?;
        if !resp.status().is_success() {
            bail!("no module found for '{r}'");
        }
        let html = resp.text()?;
        let key = "data-module-id=\"";
        let i = html
            .find(key)
            .with_context(|| format!("no module found for '{r}'"))?;
        let rest = &html[i + key.len()..];
        let id: String = rest.chars().take_while(char::is_ascii_digit).collect();
        id.parse()
            .with_context(|| format!("no module found for '{r}'"))
    }

    // ----- collection ----------------------------------------------------

    /// Returns false if the module was already in the collection.
    pub fn collection_add(&self, module_id: u64) -> Result<bool> {
        match self.ajax(
            "collections/add.json",
            &[("moduleId", module_id.to_string())],
        ) {
            Ok(_) => Ok(true),
            Err(e) if e.to_string().contains("already added") => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn collection_remove(&self, module_id: u64) -> Result<()> {
        self.ajax(
            "collections/remove.json",
            &[("moduleId", module_id.to_string())],
        )?;
        Ok(())
    }

    pub fn collection(&self) -> Result<Vec<Module>> {
        let f = SearchFilters {
            mine: true,
            ..Default::default()
        };
        Ok(self.search(&f, usize::MAX)?.1)
    }

    // ----- racks ---------------------------------------------------------

    pub fn racks(&self) -> Result<Vec<RackSummary>> {
        let (u, html) = self.get_html("racks/command_center", &[])?;
        self.require_login(&html, &u)?;
        let doc = Html::parse_document(&html);
        let mut out = vec![];
        for item in doc.select(&sel(".li-screenshot")) {
            let Some(a) = item.select(&sel("a.lnk-rack[data-rack-id]")).next() else {
                continue;
            };
            let Some(id) = a.value().attr("data-rack-id").and_then(|s| s.parse().ok()) else {
                continue;
            };
            let name = item.select(&sel("h3")).next().map(text).unwrap_or_default();
            out.push(RackSummary { id, name });
        }
        Ok(out)
    }

    /// Resolve a rack reference (id, URL, or exact/unique name) to its id.
    pub fn resolve_rack(&self, r: &str) -> Result<u64> {
        if let Ok(id) = r.parse() {
            return Ok(id);
        }
        if let Some(rest) = r.split("/racks/view/").nth(1) {
            let id: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(id) = id.parse() {
                return Ok(id);
            }
        }
        let racks = self.racks()?;
        let matches: Vec<_> = racks.iter().filter(|x| x.name == r).collect();
        match matches.as_slice() {
            [one] => Ok(one.id),
            [] => {
                let ci: Vec<_> = racks
                    .iter()
                    .filter(|x| x.name.eq_ignore_ascii_case(r))
                    .collect();
                match ci.as_slice() {
                    [one] => Ok(one.id),
                    [] => bail!("no rack named '{r}'"),
                    _ => bail!("rack name '{r}' is ambiguous; use the rack id"),
                }
            }
            _ => bail!("rack name '{r}' is ambiguous; use the rack id"),
        }
    }

    pub fn rack(&self, id: u64) -> Result<Rack> {
        let r = self
            .http
            .get(self.url(&format!("racks/view/{id}")))
            .send()?;
        let not_found = r.status() == reqwest::StatusCode::NOT_FOUND;
        let r = if not_found { r } else { r.error_for_status()? };
        let u = r.url().clone();
        let html = r.text()?;
        if not_found || !u.path().contains("/racks/view/") {
            bail!("rack {id} not found (or it's private and you're not logged in as its owner)");
        }
        let doc = Html::parse_document(&html);
        let raw = doc
            .select(&sel(r#"script[data-mg-json="rtd"]"#))
            .next()
            .map(|s| s.text().collect::<String>())
            .context("rack data not found on page")?;
        let v: Value = serde_json::from_str(&raw).context("parsing rack data")?;
        let rack = &v["rack"];
        let r = &rack["Rack"];
        let modules = rack["Module"]
            .as_array()
            .map(|a| a.iter().map(parse_rack_module).collect())
            .unwrap_or_default();
        Ok(Rack {
            id: num(&r["id"]),
            name: r["name"].as_str().unwrap_or_default().to_string(),
            rows: num(&r["rows"]) as u32,
            hp: num(&r["te"]) as u32,
            rows_1u: r["rows1u"]
                .as_array()
                .map(|a| a.iter().map(|x| num(x) as u32).collect())
                .unwrap_or_default(),
            private: r["is_private"].as_bool().unwrap_or(false),
            owner: rack["User"]["username"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            modules,
        })
    }

    /// Power, price and depth totals, as shown under the rack on the website.
    pub fn rack_totals(&self, id: u64) -> Result<RackTotals> {
        let r = self.ajax("racks/totals.json", &[("rackId", id.to_string())])?;
        let t = &r["totals"];
        let s = |v: &Value| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        };
        Ok(RackTotals {
            current_plus_12v: num(&t["current_plus"]),
            current_minus_12v: num(&t["current_min"]),
            current_5v: num(&t["current5v"]),
            modules: num(&t["qtyModules"]),
            max_depth_mm: s(&t["maxdepth"]).and_then(|d| d.parse().ok()),
            price_eur: s(&t["price_eur"]),
            price_usd: s(&t["price_usd"]),
            // This comes as HTML (module links); keep just the text.
            incomplete: s(&t["incompletesText"])
                .map(|x| {
                    Html::parse_fragment(&x)
                        .root_element()
                        .text()
                        .collect::<String>()
                })
                .filter(|x| x != "none"),
        })
    }

    pub fn create_rack(&self, nr: &NewRack) -> Result<u64> {
        let theme = match nr.theme {
            Some(t) => t.to_string(),
            None => self.default_theme()?,
        };
        let mut form: Vec<(String, String)> = vec![
            ("_method".into(), "POST".into()),
            ("data[Rack][name]".into(), nr.name.into()),
            ("data[Rack][rows]".into(), nr.rows.to_string()),
            ("data[Rack][te]".into(), nr.hp.to_string()),
            ("data[Rack][format]".into(), nr.format.into()),
            ("data[Rack][rows1u]".into(), String::new()),
        ];
        for r in nr.rows_1u {
            form.push(("data[Rack][rows1u][]".into(), r.to_string()));
        }
        form.push((
            "data[Rack][is_private]".into(),
            if nr.private { "1" } else { "0" }.into(),
        ));
        form.push(("data[Rack][url]".into(), nr.url.into()));
        form.push(("data[Rack][theme_id]".into(), theme));

        let resp = self
            .http
            .post(self.url("racks/add"))
            .form(&form)
            .send()?
            .error_for_status()?;
        let final_url = resp.url().clone();
        let html = resp.text()?;
        self.require_login(&html, &final_url)?;
        if let Some(rest) = final_url.path().split("/racks/view/").nth(1)
            && let Ok(id) = rest.trim_end_matches('/').parse()
        {
            return Ok(id);
        }
        let errors: Vec<String> = Html::parse_document(&html)
            .select(&sel(".error-message, .invalid-feedback, .alert-danger"))
            .map(text)
            .filter(|t| !t.is_empty())
            .collect();
        if errors.is_empty() {
            bail!("rack creation failed (landed on {final_url})");
        }
        let msg = errors.join("; ");
        if msg.contains("Rows") || msg.contains(" HP") {
            // Account limits are cached in the server-side session at login time.
            bail!(
                "rack creation failed: {msg}\n\
                 (free accounts are limited in rows/HP; if you've just upgraded, run \
                 `modulargrid logout && modulargrid login` so the site picks up your new account type)"
            );
        }
        bail!("rack creation failed: {msg}")
    }

    fn default_theme(&self) -> Result<String> {
        let (u, html) = self.get_html("racks/add", &[])?;
        self.require_login(&html, &u)?;
        let doc = Html::parse_document(&html);
        Ok(doc
            .select(&sel(r#"input[name="data[Rack][theme_id]"][checked]"#))
            .next()
            .and_then(|e| e.value().attr("value"))
            .unwrap_or("1")
            .to_string())
    }

    pub fn delete_rack(&self, id: u64) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("racks/delete/{id}")))
            .form(&[("_method", "POST")])
            .send()?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("rack {id} not found");
        }
        let resp = resp.error_for_status()?;
        let final_url = resp.url().clone();
        let html = resp.text()?;
        self.require_login(&html, &final_url)?;
        if html.contains("Rack deleted") {
            return Ok(());
        }
        bail!("could not delete rack {id} (does it exist and belong to you?)")
    }

    /// Add a module to a rack in the first free spot the server picks.
    pub fn rack_add(&self, rack_id: u64, module_id: u64) -> Result<RackModule> {
        let r = self.ajax(
            "modules_racks/add.json",
            &[
                ("moduleId", module_id.to_string()),
                ("rackId", rack_id.to_string()),
            ],
        )?;
        Ok(parse_rack_module(&r["module"]))
    }

    /// Move a module instance to (row, col); `col` is the 1-based HP position.
    pub fn rack_move(&self, instance_id: u64, row: u32, col: u32) -> Result<()> {
        self.ajax(
            "modules_racks/move.json",
            &[
                ("modules_rack_id", instance_id.to_string()),
                ("row", row.to_string()),
                ("col", col.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn rack_remove_instance(&self, instance_id: u64) -> Result<()> {
        self.ajax(
            "modules_racks/delete.json",
            &[("modules_rack_id", instance_id.to_string())],
        )?;
        Ok(())
    }
}

/// "HP 14" for a single HP, "HP 14-27" for a range.
fn hp_span(start: u32, end: u32) -> String {
    if start == end {
        format!("HP {start}")
    } else {
        format!("HP {start}-{end}")
    }
}

/// Numbers in the site's JSON come as either strings or numbers.
fn num(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn parse_rack_module(m: &Value) -> RackModule {
    let mr = &m["ModulesRack"];
    RackModule {
        instance_id: num(&mr["id"]),
        module_id: num(&m["id"]),
        name: m["name"].as_str().unwrap_or_default().to_string(),
        vendor: m["Vendor"]["name"].as_str().unwrap_or_default().to_string(),
        description: m["description"].as_str().unwrap_or_default().to_string(),
        hp: num(&m["te"]) as u32,
        row: num(&mr["row"]) as u32,
        col: num(&mr["col"]) as u32,
        in_bounds: mr["is_inbounds"].as_bool().unwrap_or(true),
        is_1u: m["is_1u"]
            .as_bool()
            .unwrap_or_else(|| num(&m["is_1u"]) == 1),
        current_plus_12v: num(&m["current_plus"]),
        current_minus_12v: num(&m["current_min"]),
        current_5v: num(&m["current5v"]),
    }
}

fn parse_module_box(b: ElementRef) -> Option<Module> {
    let id = b.value().attr("data-module-id")?.parse().ok()?;
    let link = b.select(&sel("h2.module-name a")).next()?;
    let slug = link
        .value()
        .attr("href")
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    Some(Module {
        id,
        name: text(link),
        slug,
        vendor: b
            .select(&sel(".vendor-name a.lnk-vendor"))
            .next()
            .map(text)
            .unwrap_or_default(),
        // Rendered as e.g. "20 HP".
        hp: b
            .select(&sel(r#"span[title="Module Width"]"#))
            .next()
            .and_then(|e| text(e).split_whitespace().next()?.parse().ok()),
        description: b
            .select(&sel(".caption p"))
            .next()
            .map(text)
            .unwrap_or_default(),
        price: b.select(&sel(".price .currency")).next().map(text),
    })
}

fn is_anonymous(html: &str) -> bool {
    // `<script type="application/json" data-mg-json="base-config">{... "is_anonymous":true}`
    html.contains(r#""is_anonymous":true"#)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}
