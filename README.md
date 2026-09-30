# `modulargrid` - ModularGrid from the command line

modulargrid is an unofficial CLI for [ModularGrid](https://modulargrid.net). It can search for modules, manage your collection, and build racks, directly in the terminal.

I built the modulargrid CLI so my agents could manage my ModularGrid racks. Take a picture of your rack and give Claude access to modulargrid, and it will build the virtual rack for you!

```console
$ modulargrid search maths -s popular -n 3
  2697   20 HP  Make Noise  Maths                Maths 2013
 20545   20 HP  Make Noise  MATHS (white knobs)  Maths 2019: Analog computer designed for musical purposes
   206   20 HP  Make Noise  Pressure Points      Touch Controller / Manual Sequencer
(3 of 42 results; use -n to show more)

$ modulargrid rack create Skiff --hp 84 --rows 1
Created rack 3227954: https://modulargrid.net/e/racks/view/3227954

$ modulargrid rack add Skiff 20545 206 7411
Added Make Noise MATHS (white knobs) (20545) at row 1, col 1 (instance 123758949).
Added Make Noise Pressure Points (206) at row 1, col 21 (instance 123758950).
Added Mutable instruments Rings (7411) at row 1, col 41 (instance 123758951).

$ modulargrid rack view Skiff
Skiff — 1 row × 84 HP (id 3227954)

Row 1 — 54/84 HP filled
  • Make Noise MATHS (white knobs) — 20 HP
  • Make Noise Pressure Points — 20 HP
  • Mutable instruments Rings — 14 HP
  ◦ empty — 30 HP

Power consumption: 200 mA +12V | 55 mA -12V | 0 mA +5V
Depth: 25 mm | Modules: 3 | Price: €813 / $840
```

ModularGrid doesn't have a public API, so modulargrid talks to the same internal endpoints the website's own JavaScript uses. Login goes through a real browser window (the login form is protected by reCAPTCHA), and modulargrid picks up the session from there.

Every command takes `--json`, which makes modulargrid easy to script and easy for agents to drive. See [Viewing a rack](#viewing-a-rack) for a full case that was built from photos.

## Table of contents

- [Installation](#installation)
- [Usage](#usage)
- [Logging in](#logging-in)
  - [Browser login](#browser-login)
  - [Pasting a cookie](#pasting-a-cookie)
  - [whoami and logout](#whoami-and-logout)
- [Searching modules](#searching-modules)
  - [Filters](#filters)
  - [Vendors and functions](#vendors-and-functions)
  - [Other/unknown vendors](#otherunknown-vendors)
- [Collection](#collection)
- [Racks](#racks)
  - [Creating and listing racks](#creating-and-listing-racks)
  - [Adding modules](#adding-modules)
  - [Exact placement](#exact-placement)
  - [Moving modules](#moving-modules)
  - [Viewing a rack](#viewing-a-rack)
  - [Removing modules](#removing-modules)
  - [Deleting racks](#deleting-racks)
- [Module and rack references](#module-and-rack-references)
- [JSON output](#json-output)
- [Other formats](#other-formats)
- [Environment variables](#environment-variables)
- [How it works](#how-it-works)
- [Development](#development)
- [License](#license)

## Installation

### Via Homebrew (macOS/Linux)

```bash
brew install andreasjansson/tap/modulargrid
```

### Pre-built binaries

Download from the [releases page](https://github.com/andreasjansson/modulargrid-cli/releases). Binaries are available for:
- Linux (x86_64, ARM64; glibc and musl)
- macOS (Intel, Apple Silicon)

### From source

```bash
cargo install --git https://github.com/andreasjansson/modulargrid-cli
```

Or from a checkout of this repository:

```bash
cargo install --path .
```

Browser login needs a Chromium-based browser: Google Chrome, Chromium, Brave, Microsoft Edge or Vivaldi.

## Usage

```
Command-line client for ModularGrid (https://modulargrid.net)

Usage: modulargrid [OPTIONS] <COMMAND>

Commands:
  search      Search for modules (same filters as the website's module browser)
  vendors     List manufacturers (optionally filtered), with the ids `search --vendor` accepts
  functions   List module functions, with the ids `search --function` accepts
  login       Log in through a browser window (or with a pasted session cookie)
  logout      Log out and forget the stored session
  whoami      Show the logged-in user
  collection  Manage your module collection
  rack        Manage racks
  help        Print this message or the help of the given subcommand(s)

Options:
      --json             Output JSON instead of human-readable text
      --format <FORMAT>  Module format / site section (e = Eurorack, u = Buchla, s = Serge, p = Pedals, ...) [env: MODULARGRID_FORMAT=] [default: e]
  -h, --help             Print help
  -V, --version          Print version
```

Searching works without logging in. Everything that touches your collection or racks needs a session.

## Logging in

### Browser login

```console
$ modulargrid login
Opening /Applications/Google Chrome.app/Contents/MacOS/Google Chrome — log in there; this window closes automatically.
(Tick "Remember me" to stay logged in longer.)
Logged in as example_user.
```

modulargrid opens a separate browser window with a fresh, temporary profile at the ModularGrid login page. Log in as usual. As soon as the session is logged in, modulargrid saves the cookies, closes the window and deletes the temporary profile. Your regular browser profile is never touched.

The session is saved to `session.json` in the modulargrid config directory, readable only by you. Tick "Remember me" to keep it valid for longer.

ModularGrid reads your account type when you log in. If you upgrade your account, log out and back in so the new limits apply.

Options:

| Option | Description |
|--------|-------------|
| `--browser <PATH>` | Browser executable to use instead of the auto-detected one |
| `--timeout <SECONDS>` | How long to wait for the login to complete (default 300) |
| `--cookie <VALUE>` | Skip the browser and use a pasted `CAKEPHP` cookie instead |

### Pasting a cookie

If you can't run a browser, for example on a headless machine, copy the `CAKEPHP` cookie from your browser's developer tools:

```bash
modulargrid login --cookie <CAKEPHP cookie value>
```

modulargrid checks the cookie before saving it:

```console
$ modulargrid login --cookie bogus123
error: login failed: the session cookie is not logged in
```

### whoami and logout

```console
$ modulargrid whoami
example_user (id 123456)

$ modulargrid logout
Logged out.

$ modulargrid whoami
error: not logged in — run `modulargrid login`
```

`logout` ends the session on ModularGrid and deletes the stored session file.

## Searching modules

```console
$ modulargrid search maths -s popular -n 5
  2697   20 HP  Make Noise  Maths                Maths 2013
 20545   20 HP  Make Noise  MATHS (white knobs)  Maths 2019: Analog computer designed for musical purposes
   206   20 HP  Make Noise  Pressure Points      Touch Controller / Manual Sequencer
  6018   20 HP  Make Noise  MATHS (black panel)  Analog Computer
   217   34 HP  Make Noise  Rene                 René - Cartesian Sequencer
(5 of 42 results; use -n to show more)
```

The columns are module id, width, vendor, name and description. The id is what you pass to `collection` and `rack` commands. Queries can be several words without quotes, e.g. `modulargrid search noodle rider`.

Results are paginated on the server. modulargrid follows the pages until it has `-n` results (default 30). The "x of y" line goes to stderr, so piping the results elsewhere stays clean.

### Filters

`search` supports every filter in the website's module browser:

| Option | Description |
|--------|-------------|
| `-v, --vendor <VENDOR>` | Manufacturer name or id |
| `-f, --function <FUNCTION>` | Function name or id, e.g. `VCA`, `Oscillator` |
| `--secondary <FUNCTION>` | Secondary function name or id |
| `--exclude-secondary` | Exclude rather than require the secondary function |
| `--hp <HP>` | Maximum width in HP |
| `--hp-exact` | Match `--hp` exactly instead of as a maximum |
| `--height <HEIGHT>` | `full`, `1u`, `1u-intellijel` or `1u-pulp` |
| `--max-depth <MM>` | Maximum depth in mm |
| `--build <BUILD>` | `assembled` or `diy` |
| `--lifecycle <LIFECYCLE>` | `concept`, `available`, `discontinued` or `unassigned` |
| `--marketplace <REGION>` | Only modules offered in a region, e.g. `EU`, `USA`, `UK`, `Global` |
| `--modeled` | Only modules that have a 3D model |
| `--passive` | Only passive modules |
| `--others` | Include modules by "Other/unknown" vendors |
| `--mine` | Only search within your collection |
| `-s, --sort <SORT>` | `newest`, `popular`, `alphabetic`, `price`, `manufacturer`, `hp`, `power`, `depth` or `functions` |
| `--desc` | Sort descending |
| `-n, --limit <N>` | Maximum number of results (default 30) |

Filters combine:

```console
$ modulargrid search -v ladik --hp 4 --hp-exact -s alphabetic -n 5
  4744    4 HP  Ladik   A-011 Dual log VCA   Dual Log VCA
  4746    4 HP  Ladik   A-012 Dual Lin VCA   Dual Linear VCA
  6535    4 HP  Ladik   A-310 Headphone Amp  mono / stereo headphone amp with active mult
 30295    4 HP  Ladik   a-312                2IN HP AMP
 32870    4 HP  Ladik   A-312 black          Dual Input Headphones Amp/Mix/Stereo line out (4HP)
(5 of 171 results; use -n to show more)

$ modulargrid search -f vca -v "make noise" -s popular -n 4
 20712   18 HP  Make Noise  QPAS               Quad Core Stereo Analog
  7821    8 HP  Make Noise  Optomix rev2 2016  2 Ch Low Pass Gate / Mixer
   204    8 HP  Make Noise  Optomix            2 Ch Low Pass Gate / Mixer
 22046   10 HP  Make Noise  X-PAN              5 channel Voltage Controlled Stereo Mixer
(4 of 16 results; use -n to show more)
```

### Vendors and functions

`--vendor`, `--function` and `--marketplace` take a name, an id, or any unique part of a name, case-insensitively. When a name is ambiguous, modulargrid lists the candidates:

```console
$ modulargrid search -v noise
error: 'noise' matches several vendors: Black Noise (967), British Noise Electronics (939), GenXnoise (1405), Make Noise (52), Mental Noise (1193), Noise Engineering (186), Noise Lab (881), Noise of Antimatter (1227), Noise Reap (352)
```

To browse the lists, use `vendors` (with an optional filter) and `functions`:

```console
$ modulargrid vendors noise
   967  Black Noise
   939  British Noise Electronics
  1405  GenXnoise
    52  Make Noise
  1193  Mental Noise
   186  Noise Engineering
   881  Noise Lab
  1227  Noise of Antimatter
   352  Noise Reap

$ modulargrid functions | head -8
    29  Attenuator
    16  Blank Panel
    36  Clock Generator
    42  Clock Modulator
    56  Comparator
    38  Controller
    15  CV Modulation
    13  Delay
```

### Other/unknown vendors

Like the website, search hides modules filed under the "Other/unknown" vendor unless you ask for them:

```console
$ modulargrid search noodle rider
No modules found (try --others to include modules by "Other/unknown" vendors).

$ modulargrid search noodle rider --others
 47550   20 HP  Other/unknown  Noodle Rider Sidecar  Quad VCA controlled by envelope generators (AD/AR), with MI…
 47549   33 HP  Other/unknown  Noodle Rider          Four-track cassette player with variable-speed playback con…
```

`--mine`, `collection list` and `collection purge` always include them, so collection listings are complete.

## Collection

```console
$ modulargrid collection add make-noise-maths- 206 https://modulargrid.net/e/mutable-instruments-rings
Module 201 added to collection.
Module 206 added to collection.
Module 7411 added to collection.

$ modulargrid collection add 206
Module 206 already in collection.

$ modulargrid collection list
  7411   14 HP  Mutable instruments  Rings            Resonator
   206   20 HP  Make Noise           Pressure Points  Touch Controller / Manual Sequencer
   201   20 HP  Make Noise           Maths            Lightning bolt generator
(3 modules)

$ modulargrid search --mine rings
  7411   14 HP  Mutable instruments  Rings  Resonator

$ modulargrid collection remove 206
Module 206 removed from collection.
```

`purge` removes everything, after asking for confirmation (`-y` skips the prompt):

```console
$ modulargrid collection purge
Remove all 2 modules from your collection? [y/N] y
[1/2] removed Mutable instruments Rings
[2/2] removed Make Noise Maths
Removed 2 modules.

$ modulargrid collection list
(0 modules)
```

ModularGrid has no "clear collection" action, so `purge` removes modules one at a time. It then lists the collection again and repeats until it's empty. If anything is left, the command fails instead of reporting success.

`col` is an alias for `collection`, and `ls`/`rm` are aliases for `list`/`remove`.

## Racks

### Creating and listing racks

```console
$ modulargrid rack create "Travel case" --hp 104 --rows 2 --private
Created rack 3227953: https://modulargrid.net/e/racks/view/3227953

$ modulargrid rack list
  3227953  Travel case
  9999001  primary
```

| Option | Description |
|--------|-------------|
| `--hp <HP>` | Width in HP (default 84) |
| `--rows <ROWS>` | Number of rows (default 2) |
| `--rows-1u <ROWS>` | Comma-separated rows (1-based) that are 1U rows |
| `--private` | Make the rack private |
| `--url <URL>` | Link to show on the rack |
| `--theme <ID>` | Theme id (defaults to the site's default) |

Free accounts are limited to 3 rows. If you hit the limit right after upgrading, log out and back in.

### Adding modules

Without a position, each module goes into the first free spot, just like clicking a module on the website:

```console
$ modulargrid rack add "Travel case" 201 206
Added Make Noise Maths (201) at row 1, col 1 (instance 123758932).
Added Make Noise Pressure Points (206) at row 1, col 21 (instance 123758933).
```

Every module you add gets its own *instance id*. Use it to move or remove that specific copy.

### Exact placement

Append `@ROW:COL` to place a module exactly. `ROW` is 1-based from the top and `COL` is the 1-based HP position from the left:

```console
$ modulargrid rack add "Travel case" 7411@2:1 20545@2:30 2697@2:40
Added Mutable instruments Rings (7411) at row 2, col 1 (instance 123758934).
Added Make Noise MATHS (white knobs) (20545) at row 2, col 30 (instance 123758935).
error: could not place Make Noise Maths (2697) at row 2, col 40: row 2, HP 40-59 overlaps Make Noise MATHS (white knobs) (instance 123758935, HP 30-49)

$ modulargrid rack add "Travel case" 7411@3:1
error: could not place Mutable instruments Rings (7411) at row 3, col 1: row 3 is outside the rack (rows 1-2)
```

ModularGrid's server accepts any position, including overlapping and off-rack ones; the website checks placement in the browser. modulargrid does the same checks itself: the rack's bounds, its row count, that 1U tiles only go in 1U rows (and full-height modules only in 3U rows), and overlaps with modules already in the rack and with earlier modules in the same command. When a placement fails, modulargrid removes the half-added module rather than leaving it in the wrong place.

### Moving modules

`rack show` lists every module with its instance id and position:

```console
$ modulargrid rack show "Travel case"
Travel case (id 3227953) — 2 rows × 104 HP, private, by example_user
  INSTANCE   MODULE  ROW   COL    HP  MODULE
 123758932      201    1     1    20  Make Noise — Maths
 123758933      206    1    21    20  Make Noise — Pressure Points
 123758934     7411    2     1    14  Mutable instruments — Rings
 123758935    20545    2    30    20  Make Noise — MATHS (white knobs)

$ modulargrid rack move "Travel case" 123758935 2 15
Moved instance 123758935 in rack 3227953 to row 2, col 15.
```

Moves are checked the same way as placements. To make room in a full row, shift modules starting from the right so that no move overlaps a module that hasn't moved yet.

### Viewing a rack

`rack view` shows each row from top to bottom: modules, blank panels and empty space, then the power consumption and other totals the website shows under the rack:

```console
$ modulargrid rack add "Travel case" 206 296@2:35 296@2:36
Added Make Noise Pressure Points (206) at row 1, col 41 (instance 123758938).
Added Doepfer A-100B1 (296) at row 2, col 35 (instance 123758939).
Added Doepfer A-100B1 (296) at row 2, col 36 (instance 123758940).

$ modulargrid rack view "Travel case"
Travel case — 2 rows × 104 HP, private (id 3227953)

Row 1 — 60/104 HP filled
  • Make Noise Maths — 20 HP
  • Make Noise Pressure Points — 20 HP
  • Make Noise Pressure Points — 20 HP
  ◦ empty — 44 HP

Row 2 — 36/104 HP filled, incl. 2 HP of blanks
  • Mutable instruments Rings — 14 HP
  • Make Noise MATHS (white knobs) — 20 HP
  • Doepfer A-100B1 — 1 HP [blank]
  • Doepfer A-100B1 — 1 HP [blank]
  ◦ empty — 68 HP

Power consumption: 280 mA +12V | 105 mA -12V | 0 mA +5V
Depth: 25 mm | Modules: 7 | Price: €1.259 / $1,341
```

Blank panels are tagged `[blank]`. Modules parked outside the rack area are listed under "Outside the rack". If ModularGrid is missing power or price data for some modules, a final line names them, since the totals will be low.

<details>
<summary>A full 5 × 168 HP case</summary>

```console
$ modulargrid rack view primary
primary — 5 rows × 168 HP (id 9999001)

Row 1 — 168/168 HP filled
  • X Audio Systems VCRadio — 8 HP
  • After Later Audio Pixie — 12 HP
  • Doepfer A-111-4v — 18 HP
  • Make Noise DPO — 28 HP
  • Mutable instruments Rings — 14 HP
  • Mutable instruments Beads — 14 HP
  • Tiptop Audio Buchla 258t — 18 HP
  • Noise Engineering Basimilus Iteritas Alia — 10 HP
  • Acid Rain Technology Ripsaw — 6 HP
  • Erica Synths CLAP — 6 HP
  • Shakmat Archer's Rig — 8 HP
  • OhmForce bohm performer — 8 HP
  • OhmForce bohm — 18 HP

Row 2 — 168/168 HP filled
  • NANO Modules ST FONT — 14 HP
  • Doepfer A-141-4 — 8 HP
  • Doepfer A-130-4 Quad VCA — 4 HP
  • Doepfer A-105-4 — 8 HP
  • Doepfer A-171-4 — 4 HP
  • Knobula Pianophonic — 12 HP
  • Make Noise Morphagene — 20 HP
  • SOMA Laboratory Lyra8-FX — 20 HP
  • Make Noise QPAS — 18 HP
  • Ladik O-410 Sub Osc — 4 HP
  • AJH Synth Minimod Transistor Ladder Filter "Dark Edition" — 14 HP
  • Strymon StarLab — 28 HP
  • Happy Nerding FX AID Pro — 14 HP

Row 3 — 168/168 HP filled
  • Make Noise MATHS (white knobs) — 20 HP
  • Xaoc Devices Zadar — 10 HP
  • 2hp VCA (Black Panel) — 2 HP
  • Make Noise Mimeophon — 16 HP
  • Doepfer A-138j — 6 HP
  • Ladik C-216 IADSR — 4 HP
  • Ladik L-121 Folding LFO — 4 HP
  • Xaoc Devices Skopje — 6 HP
  • Make Noise MultiMod — 10 HP
  • Doepfer A-142-4 — 8 HP
  • Befaco A*B+C — 6 HP
  • Nekyia Circuits Sosumi — 4 HP
  • WMD Buffered Mult (Black) — 4 HP
  • Doepfer A-183-5 — 4 HP
  • DivKid ochd — 4 HP
  • WORNG Electronics MidSide+ — 8 HP
  • Doepfer A-132-3 — 8 HP
  • Steady State Fate SSG Stereo Field — 10 HP
  • Doepfer A-106-5 SEM — 8 HP
  • Doepfer A-121-3 — 4 HP
  • Doepfer A-121-3 — 4 HP
  • Doepfer A-121-3 — 4 HP
  • Error Instruments White Rabbit — 8 HP
  • Happy Nerding FX AID XL — 6 HP

Row 4 — 168/168 HP filled, incl. 2 HP of blanks
  • Ladik S-060 VC 4ch(out) Clockworks6 — 8 HP
  • Ladik S-090  Dual probability skipper — 4 HP
  • Ladik S-050 VC 2ch(out) Clockworks5 — 4 HP
  • Ladik B-011 Sequential logic module — 4 HP
  • Joranalogue Audio Design Walk 4 — 10 HP
  • Joranalogue Audio Design Compare 2 — 8 HP
  • Doepfer A-166 — 8 HP
  • Teia SkipLog — 8 HP
  • Ladik M-100 Unity mixer w. transpose — 4 HP
  • Ladik S-075 Burst generator — 4 HP
  • Ladik S-185 Gatsby — 4 HP
  • Doepfer A-151v — 4 HP
  • Ladik R-216 ASR — 4 HP
  • Ladik B-230 Curious Goat  Black Panel — 8 HP
  • Doepfer A-148v — 4 HP
  • Doepfer A-132-3 — 8 HP
  • 2hp Mix (Black Panel) — 2 HP
  • Nonlinearcircuits Wangernumb — 14 HP
  • Befaco Percall — 12 HP
  • Happy Nerding 3x MIA — 6 HP
  • Doepfer A-100B1 — 1 HP [blank]
  • Doepfer A-100B1 — 1 HP [blank]
  • Vostok Instruments Hive — 10 HP
  • After Later Audio Ornate Criminal MIDI Expander  — 2 HP
  • After Later Audio Ornate Criminal — 10 HP
  • CalSynth XLOC2 — 16 HP

Row 5 — 168/168 HP filled, incl. 1 HP of blanks
  • ALM Busy Circuits Pamela's PRO Workout — 8 HP
  • Tiptop Audio Z8000 — 28 HP
  • Ladik S-332 Trig sequencer — 20 HP
  • Ladik S-280 — 32 HP
  • ADDAC System ADDAC304 — 8 HP
  • Joranalogue Audio Design Switch 4 — 8 HP
  • Ladik S-610 Composer N — 4 HP
  • Doepfer A-150-8 — 12 HP
  • Xaoc Devices Zlin — 3 HP
  • Intellijel Steppy — 8 HP
  • 2hp TM (Black Panel) — 2 HP
  • Doepfer A-100B1 — 1 HP [blank]
  • Half-Time Modular 8TR — 4 HP
  • After Later Audio Bartender — 24 HP
  • 4ms Company Listen IO — 6 HP

Power consumption: 5303 mA +12V | 2878 mA -12V | 0 mA +5V
Depth: 55 mm | Modules: 91 | Price: €17.503 / $19,241
Missing power/price data: XLOC2
```

</details>

`rack view` and `rack show` also work on other people's public racks by id.

### Removing modules

Remove by module (one copy at a time, starting from the bottom-right), every copy with `--all`, or a specific copy with `--instance`:

```console
$ modulargrid rack remove "Travel case" 206
Removed instance 123758938 from rack 3227953.

$ modulargrid rack remove "Travel case" 296 --all
Removed instance 123758940 from rack 3227953.
Removed instance 123758939 from rack 3227953.

$ modulargrid rack remove "Travel case" --instance 123758934
Removed instance 123758934 from rack 3227953.
```

### Deleting racks

```console
$ modulargrid rack delete "Travel case"
Delete rack 'Travel case' (3227953)? [y/N] y
Deleted rack 3227953.
```

Pass `-y` to skip the confirmation. `ls` and `rm` are aliases for `rack list` and `rack delete`.

## Module and rack references

Anywhere a command takes a module, you can use:

| Form | Example |
|------|---------|
| Numeric id | `201` |
| Slug | `make-noise-maths-` |
| URL | `https://modulargrid.net/e/make-noise-maths-` |

Anywhere a command takes a rack, you can use its numeric id, its URL (`https://modulargrid.net/e/racks/view/9999001`) or its name. Names must match exactly or case-insensitively, and must be unique among your racks.

## JSON output

Every command accepts `--json`. Progress messages and hints go to stderr, so stdout is always a single JSON document:

```console
$ modulargrid search maths -s popular -n 1 --json
{
  "modules": [
    {
      "description": "Maths 2013",
      "hp": 20,
      "id": 2697,
      "name": "Maths",
      "price": "€268",
      "slug": "make-noise-maths--",
      "vendor": "Make Noise"
    }
  ],
  "total": 42
}

$ modulargrid whoami --json
{
  "username": "example_user",
  "user_id": "123456"
}

$ modulargrid rack view primary --json | jq '.totals'
{
  "current_5v": 0,
  "current_minus_12v": 2878,
  "current_plus_12v": 5303,
  "incomplete": "XLOC2",
  "max_depth_mm": 55,
  "modules": 91,
  "price_eur": "€17.503",
  "price_usd": "$19,241"
}

$ modulargrid rack view primary --json | jq -c '.layout[4].slots[-4:][] | {kind, col, hp, name}'
{"kind":"module","col":134,"hp":1,"name":"A-100B1"}
{"kind":"module","col":135,"hp":4,"name":"8TR"}
{"kind":"module","col":139,"hp":24,"name":"Bartender"}
{"kind":"module","col":163,"hp":6,"name":"Listen IO"}
```

In `rack view --json`, each row's `slots` list includes the empty space too:

```console
$ modulargrid rack view "Travel case" --json | jq -c '.layout[0].slots[-1]'
{"col":61,"hp":44,"kind":"gap"}
```

Module slots carry per-module power (`current_plus_12v`, `current_minus_12v`, `current_5v`) and a `blank` flag.

## Other formats

ModularGrid has separate sections for other formats. Use `--format` (or `MODULARGRID_FORMAT`) to work with one of them:

| Code | Format |
|------|--------|
| `e` | Eurorack (default) |
| `c` | Modcan A |
| `m` | MOTM |
| `f` | Frac |
| `d` | Moog Unit |
| `t` | AE Modular |
| `u` | Buchla |
| `s` | Serge |
| `a` | 500 Series |
| `p` | Pedals |

```bash
modulargrid --format u search 258
```

## Environment variables

| Variable | Description |
|----------|-------------|
| `MODULARGRID_CONFIG_DIR` | Where to store `session.json` (default: `~/Library/Application Support/modulargrid` on macOS, `~/.config/modulargrid` on Linux) |
| `MODULARGRID_BROWSER` | Browser executable for `login` |
| `MODULARGRID_FORMAT` | Default for `--format` |
| `MODULARGRID_DEBUG` | Set to anything to print what the login poller is doing |

## How it works

ModularGrid is a CakePHP app. Most interactive actions on the site are AJAX calls to `/<format>/<controller>/<action>.json`, which return `{"response": {"success": ..., "result" | "msg": ...}}`. The rest are HTML pages and form posts. modulargrid uses these:

| Action | Endpoint |
|--------|----------|
| Search | `GET /e/modules/find` (HTML fragment, paginated) |
| Vendor, function and marketplace lists | `GET /e/modules/browser` (options of the search form) |
| Who am I | `GET /e/users/view` |
| Logout | `GET /e/users/logout` |
| Collection add/remove | `GET /e/collections/{add,remove}.json?moduleId=` |
| List racks | `GET /e/racks/command_center` |
| Rack contents | `GET /e/racks/view/{id}` (JSON embedded in `<script data-mg-json="rtd">`) |
| Rack totals | `GET /e/racks/totals.json?rackId=` |
| Create rack | `POST /e/racks/add` |
| Delete rack | `POST /e/racks/delete/{id}` |
| Add module to rack | `GET /e/modules_racks/add.json?moduleId=&rackId=` |
| Move module | `GET /e/modules_racks/move.json?modules_rack_id=&row=&col=` |
| Remove module from rack | `GET /e/modules_racks/delete.json?modules_rack_id=` |

Browser login uses the Chrome DevTools Protocol. modulargrid launches the browser with `--remote-debugging-port=0` and a throwaway `--user-data-dir`, then connects to the port the browser reports. It polls the `modulargrid.net` cookies until one of them is a logged-in session.

Since these endpoints aren't a public API, they can change without notice.

## Development

```bash
cargo build
cargo clippy
```

The code is split into four files:

- `src/main.rs`: command-line interface and output formatting
- `src/client.rs`: HTTP client and HTML/JSON parsing for the endpoints above
- `src/browser.rs`: browser launch and DevTools cookie extraction for `login`
- `src/session.rs`: the stored session

## License

MIT
