mod browser;
mod client;
mod session;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use client::{Client, Module, NewRack};
use session::{Cookie, Session};

/// Command-line client for ModularGrid (https://modulargrid.net).
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Output JSON instead of human-readable text.
    #[arg(long, global = true)]
    json: bool,

    /// Module format / site section (e = Eurorack, u = Buchla, s = Serge, p = Pedals, ...).
    #[arg(long, global = true, default_value = "e", env = "MODULARGRID_FORMAT")]
    format: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Search for modules (same filters as the website's module browser).
    Search(SearchArgs),
    /// List manufacturers (optionally filtered), with the ids `search --vendor` accepts.
    Vendors {
        /// Substring to filter by.
        filter: Option<String>,
    },
    /// List module functions, with the ids `search --function` accepts.
    Functions,
    /// Log in through a browser window (or with a pasted session cookie).
    Login(LoginArgs),
    /// Log out and forget the stored session.
    Logout,
    /// Show the logged-in user.
    Whoami,
    /// Manage your module collection.
    #[command(subcommand, alias = "col")]
    Collection(CollectionCmd),
    /// Manage racks.
    #[command(subcommand)]
    Rack(RackCmd),
}

#[derive(Args)]
struct SearchArgs {
    /// Name to search for (empty lists everything).
    query: Vec<String>,
    /// Manufacturer name or id (see `modulargrid vendors`).
    #[arg(short, long)]
    vendor: Option<String>,
    /// Function name or id, e.g. "VCA" (see `modulargrid functions`).
    #[arg(short, long)]
    function: Option<String>,
    /// Secondary function name or id.
    #[arg(long, value_name = "FUNCTION")]
    secondary: Option<String>,
    /// Exclude (rather than require) the secondary function.
    #[arg(long, requires = "secondary")]
    exclude_secondary: bool,
    /// Width in HP (maximum, or exact with --hp-exact).
    #[arg(long)]
    hp: Option<u32>,
    /// Match --hp exactly instead of as a maximum.
    #[arg(long, requires = "hp")]
    hp_exact: bool,
    /// Module height.
    #[arg(long, value_enum)]
    height: Option<Height>,
    /// Maximum depth in mm.
    #[arg(long, value_name = "MM")]
    max_depth: Option<u32>,
    /// Build type.
    #[arg(long, value_enum)]
    build: Option<Build>,
    /// Lifecycle status.
    #[arg(long, value_enum)]
    lifecycle: Option<Lifecycle>,
    /// Only modules offered in this marketplace region (e.g. EU, USA, UK, Global).
    #[arg(long, value_name = "REGION")]
    marketplace: Option<String>,
    /// Only modules that have a 3D model.
    #[arg(long)]
    modeled: bool,
    /// Only passive modules.
    #[arg(long)]
    passive: bool,
    /// Include modules filed under the "Other/unknown" vendor (hidden by default on the site).
    #[arg(long)]
    others: bool,
    /// Only search within your collection.
    #[arg(long)]
    mine: bool,
    /// Sort order.
    #[arg(short, long, value_enum)]
    sort: Option<Sort>,
    /// Sort descending.
    #[arg(long)]
    desc: bool,
    /// Maximum number of results.
    #[arg(short = 'n', long, default_value_t = 30)]
    limit: usize,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Height {
    /// Full-size (3U) modules only.
    Full,
    /// 1U tiles, all formats.
    #[value(name = "1u")]
    OneU,
    /// 1U tiles, Intellijel format.
    #[value(name = "1u-intellijel")]
    OneUIntellijel,
    /// 1U tiles, Pulp Logic format.
    #[value(name = "1u-pulp")]
    OneUPulp,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Build {
    Assembled,
    Diy,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Lifecycle {
    Concept,
    Available,
    Discontinued,
    Unassigned,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Sort {
    Newest,
    Popular,
    Alphabetic,
    Price,
    Manufacturer,
    Hp,
    Power,
    Depth,
    Functions,
}

impl SearchArgs {
    fn filters(&self, client: &Client) -> Result<client::SearchFilters> {
        let pick = |v: &Option<String>, list: &str, what: &str| -> Result<Option<u64>> {
            v.as_deref()
                .map(|s| client.resolve_choice(list, what, s))
                .transpose()
        };
        Ok(client::SearchFilters {
            name: self.query.join(" "),
            vendor: pick(&self.vendor, "SearchVendor", "vendor")?,
            function: pick(&self.function, "SearchFunction", "function")?,
            secondary_function: pick(&self.secondary, "SearchSecondaryfunction", "function")?,
            exclude_secondary: self.exclude_secondary,
            height: self.height.map(|h| {
                match h {
                    Height::Full => "f",
                    Height::OneU => "h",
                    Height::OneUIntellijel => "hij",
                    Height::OneUPulp => "hpl",
                }
                .to_string()
            }),
            hp: self.hp,
            hp_exact: self.hp_exact,
            max_depth: self.max_depth,
            build: self.build.map(|b| match b {
                Build::Assembled => "a".into(),
                Build::Diy => "d".into(),
            }),
            lifecycle: self.lifecycle.map(|l| {
                match l {
                    Lifecycle::Concept => "concept",
                    Lifecycle::Available => "available",
                    Lifecycle::Discontinued => "discontinued",
                    Lifecycle::Unassigned => "unassigned",
                }
                .to_string()
            }),
            mine: self.mine,
            marketplace: pick(&self.marketplace, "SearchMarketplace", "marketplace")?,
            modeled: self.modeled,
            others: self.others,
            passive: self.passive,
            order: self.sort.map(|s| {
                match s {
                    Sort::Newest => "newest",
                    Sort::Popular => "popular",
                    Sort::Alphabetic => "alphabetic",
                    Sort::Price => "price",
                    Sort::Manufacturer => "manuf",
                    Sort::Hp => "hp",
                    Sort::Power => "power",
                    Sort::Depth => "depth",
                    Sort::Functions => "tag",
                }
                .to_string()
            }),
            desc: self.desc,
        })
    }
}

#[derive(Args)]
struct LoginArgs {
    /// Use this CAKEPHP session cookie value instead of opening a browser.
    #[arg(long)]
    cookie: Option<String>,
    /// Path to a Chromium-based browser (Chrome, Chromium, Brave, Edge).
    #[arg(long, env = "MODULARGRID_BROWSER")]
    browser: Option<PathBuf>,
    /// Seconds to wait for the browser login to complete.
    #[arg(long, default_value_t = 300)]
    timeout: u64,
}

#[derive(Subcommand)]
enum CollectionCmd {
    /// List modules in your collection.
    #[command(alias = "ls")]
    List,
    /// Add modules (id, slug, or URL) to your collection.
    Add {
        #[arg(required = true)]
        modules: Vec<String>,
    },
    /// Remove modules (id, slug, or URL) from your collection.
    #[command(alias = "rm")]
    Remove {
        #[arg(required = true)]
        modules: Vec<String>,
    },
    /// Remove every module from your collection.
    Purge {
        /// Don't ask for confirmation.
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum RackCmd {
    /// List your racks.
    #[command(alias = "ls")]
    List,
    /// Show a rack's modules as a table, with the instance ids `rack move`/`remove` use.
    Show { rack: String },
    /// View a rack row by row (modules, gaps, blanks) with power consumption totals.
    View { rack: String },
    /// Create a new rack.
    Create {
        name: String,
        /// Width in HP.
        #[arg(long, default_value_t = 84)]
        hp: u32,
        /// Number of rows.
        #[arg(long, default_value_t = 2)]
        rows: u32,
        /// Rows (1-based) that are 1U rows, e.g. --rows-1u 2.
        #[arg(long = "rows-1u", value_delimiter = ',')]
        rows_1u: Vec<u32>,
        /// Make the rack private.
        #[arg(long)]
        private: bool,
        /// Link to add to the rack.
        #[arg(long, default_value = "")]
        url: String,
        /// Theme id (defaults to the site's default).
        #[arg(long)]
        theme: Option<u32>,
    },
    /// Delete a rack.
    #[command(alias = "rm")]
    Delete {
        /// Rack id, URL, or name.
        rack: String,
        /// Don't ask for confirmation.
        #[arg(short, long)]
        yes: bool,
    },
    /// Add modules to a rack.
    ///
    /// Each module goes in the first free spot, unless given as MODULE@ROW:COL
    /// (COL is the 1-based HP position from the left), e.g. `201@3:1`.
    Add {
        /// Rack id, URL, or name.
        rack: String,
        /// Module ids, slugs, or URLs, optionally suffixed with @ROW:COL.
        #[arg(required = true)]
        modules: Vec<String>,
    },
    /// Move a module instance (see `rack show`) to ROW, COL (1-based HP position).
    Move {
        /// Rack id, URL, or name.
        rack: String,
        instance: u64,
        row: u32,
        col: u32,
    },
    /// Remove modules from a rack.
    Remove {
        /// Rack id, URL, or name.
        rack: String,
        /// Module ids, slugs, or URLs. Removes one instance each unless --all.
        modules: Vec<String>,
        /// Remove all instances of the given modules.
        #[arg(long)]
        all: bool,
        /// Remove specific module instances by instance id (see `rack show`).
        #[arg(long = "instance", value_name = "ID")]
        instances: Vec<u64>,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let session = Session::load()?;
    let client = Client::new(session.as_ref(), &cli.format)?;
    let json = cli.json;

    match cli.cmd {
        Cmd::Vendors { filter } => {
            let lc = filter.unwrap_or_default().to_lowercase();
            let list: Vec<_> = client
                .choices("SearchVendor")?
                .into_iter()
                .filter(|c| c.name.to_lowercase().contains(&lc))
                .collect();
            print_choices(&list, json)?;
        }
        Cmd::Functions => print_choices(&client.choices("SearchFunction")?, json)?,
        Cmd::Search(args) => {
            if args.mine {
                require_session(&session)?;
            }
            let (total, mods) = client.search(&args.filters(&client)?, args.limit)?;
            if json {
                print_json(&serde_json::json!({"total": total, "modules": mods}))?;
            } else if mods.is_empty() {
                let hint = if args.others || args.mine {
                    ""
                } else {
                    " (try --others to include modules by \"Other/unknown\" vendors)"
                };
                eprintln!("No modules found{hint}.");
            } else {
                print_modules(&mods);
                if total > mods.len() {
                    eprintln!("({} of {total} results; use -n to show more)", mods.len());
                }
            }
        }
        Cmd::Login(args) => login(args, &cli.format, json)?,
        Cmd::Logout => {
            if session.is_some() {
                let _ = client.logout();
            }
            let existed = Session::delete()?;
            if !json {
                println!(
                    "{}",
                    if existed {
                        "Logged out."
                    } else {
                        "Not logged in."
                    }
                );
            } else {
                print_json(&serde_json::json!({"logged_out": existed}))?;
            }
        }
        Cmd::Whoami => {
            let me = if session.is_some() {
                client.whoami()?
            } else {
                None
            };
            match (me, json) {
                (Some(me), true) => print_json(&me)?,
                (Some(me), false) => match &me.user_id {
                    Some(id) => println!("{} (id {id})", me.username),
                    None => println!("{}", me.username),
                },
                (None, true) => print_json(&serde_json::Value::Null)?,
                (None, false) => bail!("not logged in — run `modulargrid login`"),
            }
            persist(&client, session)?;
        }
        Cmd::Collection(c) => {
            require_session(&session)?;
            collection(&client, c, json)?;
            persist(&client, session)?;
        }
        Cmd::Rack(c) => {
            require_session(&session)?;
            rack(&client, c, &cli.format, json)?;
            persist(&client, session)?;
        }
    }
    Ok(())
}

fn require_session(s: &Option<Session>) -> Result<()> {
    if s.is_none() {
        bail!("not logged in — run `modulargrid login`");
    }
    Ok(())
}

/// Save any cookies the server refreshed during this run.
fn persist(client: &Client, session: Option<Session>) -> Result<()> {
    if let Some(mut s) = session {
        let cookies = client.cookies();
        if !cookies.is_empty() {
            s.cookies = cookies;
            s.save()?;
        }
    }
    Ok(())
}

fn login(args: LoginArgs, format: &str, json: bool) -> Result<()> {
    if let Some(value) = args.cookie {
        let session = Session {
            cookies: vec![Cookie {
                name: "CAKEPHP".into(),
                value,
            }],
            ..Default::default()
        };
        return finish_login(session, format, json);
    }

    let exe = browser::find_browser(args.browser.as_deref())?;
    let login_url = format!("{}/{format}/users/login", client::ORIGIN);
    eprintln!(
        "Opening {} — log in there; this window closes automatically.",
        exe.display()
    );
    eprintln!("(Tick \"Remember me\" to stay logged in longer.)");
    let mut b = browser::Browser::launch(&exe, &login_url)?;

    let deadline = Instant::now() + Duration::from_secs(args.timeout);
    let mut last_session_value = String::new();
    let mut polls = 0u32;
    let result = loop {
        std::thread::sleep(Duration::from_millis(1500));
        if !b.alive() {
            break Err(anyhow::anyhow!("browser was closed before login completed"));
        }
        if Instant::now() > deadline {
            break Err(anyhow::anyhow!("timed out waiting for login"));
        }
        let cookies = match b.cookies_for("modulargrid.net") {
            Ok(c) => c,
            Err(e) => {
                debug(format_args!("reading cookies failed: {e:#}"));
                continue;
            }
        };
        debug(format_args!(
            "cookies: {}",
            cookies
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        // Check immediately when the session cookie changes (it's usually regenerated on login),
        // and otherwise every few seconds in case it isn't.
        let Some(sess) = cookies.iter().find(|c| c.name == "CAKEPHP") else {
            continue;
        };
        polls += 1;
        if sess.value == last_session_value && !polls.is_multiple_of(3) {
            continue;
        }
        let candidate = Session {
            cookies: cookies.clone(),
            ..Default::default()
        };
        let c = Client::new(Some(&candidate), format)?;
        match c.whoami() {
            Ok(Some(_)) => break Ok(candidate),
            Ok(None) => debug(format_args!("session not logged in yet")),
            Err(e) => debug(format_args!("login check failed: {e:#}")),
        }
        last_session_value = sess.value.clone();
    };
    b.close();
    finish_login(result?, format, json)
}

fn debug(args: std::fmt::Arguments) {
    if std::env::var_os("MODULARGRID_DEBUG").is_some() {
        eprintln!("[debug] {args}");
    }
}

fn finish_login(mut session: Session, format: &str, json: bool) -> Result<()> {
    let c = Client::new(Some(&session), format)?;
    let Some(me) = c.whoami()? else {
        bail!("login failed: the session cookie is not logged in");
    };
    session.username = Some(me.username.clone());
    session.user_id = me.user_id.clone();
    let refreshed = c.cookies();
    if !refreshed.is_empty() {
        session.cookies = refreshed;
    }
    session.save()?;
    if json {
        print_json(&me)?;
    } else {
        println!("Logged in as {}.", me.username);
    }
    Ok(())
}

fn collection(client: &Client, cmd: CollectionCmd, json: bool) -> Result<()> {
    match cmd {
        CollectionCmd::List => {
            let mods = client.collection()?;
            if json {
                print_json(&mods)?;
            } else {
                print_modules(&mods);
                eprintln!("({} modules)", mods.len());
            }
        }
        CollectionCmd::Add { modules } => for_each_module(client, &modules, json, |id| {
            Ok(if client.collection_add(id)? {
                "added to collection"
            } else {
                "already in collection"
            })
        })?,
        CollectionCmd::Remove { modules } => for_each_module(client, &modules, json, |id| {
            client.collection_remove(id)?;
            Ok("removed from collection")
        })?,
        CollectionCmd::Purge { yes } => {
            let mods = client.collection()?;
            if mods.is_empty() {
                println!("Collection is already empty.");
                return Ok(());
            }
            if !yes
                && !confirm(&format!(
                    "Remove all {} modules from your collection?",
                    mods.len()
                ))?
            {
                bail!("aborted");
            }
            // The collection listing is a paginated search, so re-list after each pass
            // and keep going until it comes back empty.
            let mut removed = 0;
            let mut failed = std::collections::HashSet::new();
            let mut mods = mods;
            for _pass in 0..5 {
                let todo: Vec<_> = mods.iter().filter(|m| !failed.contains(&m.id)).collect();
                if todo.is_empty() {
                    break;
                }
                for (i, m) in todo.iter().enumerate() {
                    match client.collection_remove(m.id) {
                        Ok(()) => {
                            removed += 1;
                            if !json {
                                eprintln!(
                                    "[{}/{}] removed {} {}",
                                    i + 1,
                                    todo.len(),
                                    m.vendor,
                                    m.name
                                );
                            }
                        }
                        Err(e) => {
                            failed.insert(m.id);
                            eprintln!(
                                "[{}/{}] failed to remove {} ({}): {e:#}",
                                i + 1,
                                todo.len(),
                                m.name,
                                m.id
                            );
                        }
                    }
                }
                mods = client.collection()?;
            }
            let remaining = mods.len();
            if json {
                print_json(&serde_json::json!({"removed": removed, "remaining": remaining}))?;
            } else {
                println!("Removed {removed} modules.");
            }
            if remaining > 0 {
                bail!("{remaining} module(s) are still in the collection");
            }
        }
    }
    Ok(())
}

fn rack(client: &Client, cmd: RackCmd, format: &str, json: bool) -> Result<()> {
    match cmd {
        RackCmd::List => {
            let racks = client.racks()?;
            if json {
                print_json(&racks)?;
            } else {
                for r in &racks {
                    println!("{:>9}  {}", r.id, r.name);
                }
            }
        }
        RackCmd::Show { rack } => {
            let r = client.rack(client.resolve_rack(&rack)?)?;
            if json {
                print_json(&r)?;
            } else {
                println!(
                    "{} (id {}) — {} × {} HP{}, by {}",
                    r.name,
                    r.id,
                    rows_label(r.rows),
                    r.hp,
                    if r.private { ", private" } else { "" },
                    r.owner
                );
                let mut mods: Vec<_> = r.modules.iter().collect();
                mods.sort_by_key(|m| (m.row, m.col));
                if !mods.is_empty() {
                    println!(
                        "{:>10}  {:>7}  {:>3}  {:>4}  {:>4}  MODULE",
                        "INSTANCE", "MODULE", "ROW", "COL", "HP"
                    );
                }
                for m in mods {
                    println!(
                        "{:>10}  {:>7}  {:>3}  {:>4}  {:>4}  {} — {}",
                        m.instance_id, m.module_id, m.row, m.col, m.hp, m.vendor, m.name
                    );
                }
            }
        }
        RackCmd::Create {
            name,
            hp,
            rows,
            rows_1u,
            private,
            url,
            theme,
        } => {
            let id = client.create_rack(&NewRack {
                name: &name,
                hp,
                rows,
                rows_1u: &rows_1u,
                private,
                format,
                url: &url,
                theme,
            })?;
            if json {
                print_json(&serde_json::json!({"id": id, "name": name}))?;
            } else {
                println!(
                    "Created rack {id}: {}/{format}/racks/view/{id}",
                    client::ORIGIN
                );
            }
        }
        RackCmd::Delete { rack, yes } => {
            let id = client.resolve_rack(&rack)?;
            if !yes {
                let name = client
                    .rack(id)
                    .map(|r| r.name)
                    .unwrap_or_else(|_| id.to_string());
                if !confirm(&format!("Delete rack '{name}' ({id})?"))? {
                    bail!("aborted");
                }
            }
            client.delete_rack(id)?;
            if json {
                print_json(&serde_json::json!({"deleted": id}))?;
            } else {
                println!("Deleted rack {id}.");
            }
        }
        RackCmd::Add { rack, modules } => {
            let rack_id = client.resolve_rack(&rack)?;
            // Validate every reference and position before touching the rack.
            let mut plan = vec![];
            for m in &modules {
                let (m, pos) = parse_placement(m)?;
                plan.push((client.resolve_module(m)?, pos));
            }
            let mut layout = client.rack(rack_id)?;
            let mut results = vec![];
            for (mid, pos) in plan {
                let mut added = client.rack_add(rack_id, mid)?;
                if let Some((r, c)) = pos
                    && (r, c) != (added.row, added.col)
                {
                    let placed = layout
                        .check_fit(&added, r, c)
                        .and_then(|()| client.rack_move(added.instance_id, r, c));
                    if let Err(e) = placed {
                        // Don't leave it somewhere the user didn't ask for.
                        let _ = client.rack_remove_instance(added.instance_id);
                        bail!(
                            "could not place {} {} ({mid}) at row {r}, col {c}: {e:#}",
                            added.vendor,
                            added.name
                        );
                    }
                    (added.row, added.col) = (r, c);
                }
                results.push(serde_json::json!({
                    "module_id": mid, "instance_id": added.instance_id, "row": added.row, "col": added.col
                }));
                if !json {
                    println!(
                        "Added {} {} ({mid}) at row {}, col {} (instance {}).",
                        added.vendor, added.name, added.row, added.col, added.instance_id
                    );
                }
                layout.modules.push(added);
            }
            if json {
                print_json(&results)?;
            }
        }
        RackCmd::View { rack } => {
            let r = client.rack(client.resolve_rack(&rack)?)?;
            let totals = client.rack_totals(r.id)?;
            view_rack(&r, &totals, json)?;
        }
        RackCmd::Move {
            rack,
            instance,
            row,
            col,
        } => {
            let rack_id = client.resolve_rack(&rack)?;
            let layout = client.rack(rack_id)?;
            let Some(m) = layout.modules.iter().find(|m| m.instance_id == instance) else {
                bail!("instance {instance} is not in rack {rack_id} (see `modulargrid rack show`)");
            };
            layout.check_fit(m, row, col)?;
            client.rack_move(instance, row, col)?;
            if json {
                print_json(&serde_json::json!({"instance_id": instance, "row": row, "col": col}))?;
            } else {
                println!("Moved instance {instance} in rack {rack_id} to row {row}, col {col}.");
            }
        }
        RackCmd::Remove {
            rack,
            modules,
            all,
            instances,
        } => {
            if modules.is_empty() && instances.is_empty() {
                bail!("give module ids/slugs or --instance ids to remove");
            }
            let rack_id = client.resolve_rack(&rack)?;
            let mut to_remove: Vec<u64> = instances;
            if !modules.is_empty() {
                let r = client.rack(rack_id)?;
                let mut used = std::collections::HashSet::new();
                for m in &modules {
                    let mid = client.resolve_module(m)?;
                    // Prefer removing the right-/bottom-most instance first.
                    let mut cands: Vec<_> = r
                        .modules
                        .iter()
                        .filter(|x| x.module_id == mid && !used.contains(&x.instance_id))
                        .collect();
                    cands.sort_by_key(|x| std::cmp::Reverse((x.row, x.col)));
                    if cands.is_empty() {
                        bail!("module {m} is not in rack {rack_id}");
                    }
                    let take = if all { cands.len() } else { 1 };
                    for c in cands.into_iter().take(take) {
                        used.insert(c.instance_id);
                        to_remove.push(c.instance_id);
                    }
                }
            }
            for inst in &to_remove {
                client.rack_remove_instance(*inst)?;
                if !json {
                    println!("Removed instance {inst} from rack {rack_id}.");
                }
            }
            if json {
                print_json(&serde_json::json!({"removed": to_remove}))?;
            }
        }
    }
    Ok(())
}

fn for_each_module(
    client: &Client,
    refs: &[String],
    json: bool,
    f: impl Fn(u64) -> Result<&'static str>,
) -> Result<()> {
    let mut ok = vec![];
    let mut failed = 0;
    for r in refs {
        let res = client
            .resolve_module(r)
            .and_then(|id| f(id).map(|verb| (id, verb)));
        match res {
            Ok((id, verb)) => {
                if !json {
                    println!("Module {id} {verb}.");
                }
                ok.push(id);
            }
            Err(e) => {
                failed += 1;
                eprintln!("{r}: {e:#}");
            }
        }
    }
    if json {
        print_json(&serde_json::json!({"ok": ok, "failed": failed}))?;
    }
    if failed > 0 {
        bail!("{failed} module(s) failed");
    }
    Ok(())
}

/// One entry in a rack row: a module, or a run of empty HP.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Slot<'a> {
    Module {
        #[serde(flatten)]
        module: &'a client::RackModule,
        blank: bool,
    },
    Gap {
        col: u32,
        hp: u32,
    },
}

fn rows_label(n: u32) -> String {
    format!("{n} row{}", if n == 1 { "" } else { "s" })
}

fn is_blank(m: &client::RackModule) -> bool {
    [&m.name, &m.description].iter().any(|s| {
        let s = s.to_lowercase();
        s.contains("blank") || s.contains("blind panel")
    })
}

fn row_slots(r: &client::Rack, row: u32) -> Vec<Slot<'_>> {
    let mut mods: Vec<_> = r
        .modules
        .iter()
        .filter(|m| m.in_bounds && m.row == row)
        .collect();
    mods.sort_by_key(|m| m.col);
    let mut slots = vec![];
    let mut next = 1; // first HP position not yet covered
    for m in mods {
        if m.col > next {
            slots.push(Slot::Gap {
                col: next,
                hp: m.col - next,
            });
        }
        slots.push(Slot::Module {
            module: m,
            blank: is_blank(m),
        });
        next = next.max(m.col + m.width());
    }
    if next <= r.hp {
        slots.push(Slot::Gap {
            col: next,
            hp: r.hp - next + 1,
        });
    }
    slots
}

fn view_rack(r: &client::Rack, t: &client::RackTotals, json: bool) -> Result<()> {
    if json {
        let rows: Vec<_> = (1..=r.rows)
            .map(|row| serde_json::json!({"row": row, "is_1u": r.rows_1u.contains(&row), "slots": row_slots(r, row)}))
            .collect();
        let outside: Vec<_> = r.modules.iter().filter(|m| !m.in_bounds).collect();
        return print_json(&serde_json::json!({
            "id": r.id, "name": r.name, "rows": r.rows, "hp": r.hp, "private": r.private,
            "layout": rows, "outside_rack": outside, "totals": t,
        }));
    }

    println!(
        "{} — {} × {} HP{} (id {})",
        r.name,
        rows_label(r.rows),
        r.hp,
        if r.private { ", private" } else { "" },
        r.id
    );
    for row in 1..=r.rows {
        let slots = row_slots(r, row);
        let (mut filled, mut blanks) = (0, 0);
        for s in &slots {
            if let Slot::Module { module: m, blank } = s {
                filled += m.width();
                if *blank {
                    blanks += m.width();
                }
            }
        }
        let one_u = if r.rows_1u.contains(&row) {
            " (1U)"
        } else {
            ""
        };
        let blank_note = if blanks > 0 {
            format!(", incl. {blanks} HP of blanks")
        } else {
            String::new()
        };
        println!(
            "\nRow {row}{one_u} — {filled}/{} HP filled{blank_note}",
            r.hp
        );
        for s in &slots {
            match s {
                Slot::Module { module: m, blank } => {
                    let tag = if *blank { " [blank]" } else { "" };
                    println!("  • {} {} — {} HP{tag}", m.vendor, m.name, m.width());
                }
                Slot::Gap { hp, .. } => println!("  ◦ empty — {hp} HP"),
            }
        }
    }
    let outside: Vec<_> = r.modules.iter().filter(|m| !m.in_bounds).collect();
    if !outside.is_empty() {
        println!("\nOutside the rack");
        for m in outside {
            println!("  • {} {} — {} HP", m.vendor, m.name, m.width());
        }
    }

    println!(
        "\nPower consumption: {} mA +12V | {} mA -12V | {} mA +5V",
        t.current_plus_12v, t.current_minus_12v, t.current_5v
    );
    let mut stats = vec![];
    if let Some(d) = t.max_depth_mm {
        stats.push(format!("Depth: {d} mm"));
    }
    stats.push(format!("Modules: {}", t.modules));
    match (&t.price_eur, &t.price_usd) {
        (Some(e), Some(u)) => stats.push(format!("Price: {e} / {u}")),
        (Some(p), None) | (None, Some(p)) => stats.push(format!("Price: {p}")),
        _ => {}
    }
    println!("{}", stats.join(" | "));
    if let Some(i) = &t.incomplete {
        println!("Missing power/price data: {i}");
    }
    Ok(())
}

/// Split `MODULE@ROW:COL` into the module reference and optional position.
fn parse_placement(s: &str) -> Result<(&str, Option<(u32, u32)>)> {
    let Some((m, pos)) = s.rsplit_once('@') else {
        return Ok((s, None));
    };
    let parsed = pos
        .split_once(':')
        .and_then(|(r, c)| Some((r.parse().ok()?, c.parse().ok()?)));
    match parsed {
        Some((r, c)) if r >= 1 && c >= 1 => Ok((m, Some((r, c)))),
        _ => bail!("bad position in '{s}': expected MODULE@ROW:COL with 1-based numbers"),
    }
}

fn print_choices(list: &[client::Choice], json: bool) -> Result<()> {
    if json {
        return print_json(&list);
    }
    for c in list {
        println!("{:>6}  {}", c.id, c.name);
    }
    Ok(())
}

fn print_modules(mods: &[Module]) {
    let w_name = mods
        .iter()
        .map(|m| m.name.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 40);
    let w_vendor = mods
        .iter()
        .map(|m| m.vendor.chars().count())
        .max()
        .unwrap_or(6)
        .clamp(6, 28);
    for m in mods {
        println!(
            "{:>6}  {:>6}  {:<w_vendor$}  {:<w_name$}  {}",
            m.id,
            m.hp.map(|hp| format!("{hp} HP"))
                .unwrap_or_else(|| "-".into()),
            clip(&m.vendor, w_vendor),
            clip(&m.name, w_name),
            clip(&m.description, 60),
        );
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}
