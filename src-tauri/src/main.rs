// Rust-Teil des Tool Launchers (Tauri): kennt die Programme, lädt neue
// Versionen herunter und startet sie. Die Oberfläche (ui/) bleibt HTML/JS/CSS.
//
// Aufbau neben dem Launcher (etwa auf dem USB-Stick):
//   Programme/<Ordner>/   das Programm selbst + version.json (wird ersetzt)
//   Daten/<Ordner>/       dessen Daten (bleiben immer)

// Unter Windows kein zusätzliches Konsolenfenster
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, State};
use tokio::io::AsyncWriteExt;

mod eingebettet {
    include!(concat!(env!("OUT_DIR"), "/eingebettet.rs"));
}

const LAUNCHER_REPO: &str = "Marcel1853/tool-launcher";
// Die Programmliste kommt aus dem öffentlichen Repo, damit neue Programme ohne
// neuen Launcher dazukommen. Ohne Internet gilt die mitgelieferte Liste.
const APPS_URL: &str = "https://raw.githubusercontent.com/Marcel1853/tool-launcher/main/apps.json";
// Nach einem Selbst-Update startet die neue Version mit dem Pfad der alten.
const ALT_ARG: &str = "--alter-launcher=";
#[cfg(target_os = "windows")]
const SYSTEM: &str = "win32";
#[cfg(not(target_os = "windows"))]
const SYSTEM: &str = "linux";

// --- Programmliste (apps.json) -------------------------------------------------

#[derive(Deserialize, Clone, Debug)]
struct Umzug {
    von: Vec<String>,
    dateien: Vec<String>,
    #[serde(default)]
    merkmal: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Debug)]
struct Programm {
    id: String,
    name: String,
    #[serde(default)]
    beschreibung: String,
    #[serde(default)]
    icon: Option<String>,
    repo: String,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    download: HashMap<String, String>,
    #[serde(default)]
    dateien: HashMap<String, String>,
    ordner: String,
    daten: String,
    #[serde(default)]
    umzug: Option<Umzug>,
}

#[derive(Deserialize)]
struct Liste {
    programme: Vec<Programm>,
}

#[derive(Clone, Debug)]
struct Datei {
    name: String,
    groesse: Option<u64>,
    url: String,
}

#[derive(Clone, Debug)]
struct Neueste {
    version: String,
    text: String,
    seite: String,
    datei: Option<Datei>,
}

#[derive(Default)]
struct Zustand {
    programme: Vec<Programm>,
    neueste: HashMap<String, Neueste>,
    symbole: HashMap<String, Option<String>>,
}

struct App {
    zustand: Mutex<Zustand>,
    wurzel: PathBuf,
    alter_launcher: Option<PathBuf>,
    alte_geloescht: Mutex<Option<String>>,
}

// --- Ordner ----------------------------------------------------------------

fn wurzel_ordner() -> PathBuf {
    // Zum Testen: eigener Ordner statt neben dem Programm
    if let Some(d) = std::env::var_os("LAUNCHER_ROOT") {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("PORTABLE_EXECUTABLE_DIR") {
        return PathBuf::from(d);
    }
    // Linux, AppImage: der Ordner, in dem die .AppImage-Datei liegt
    if let Some(a) = std::env::var_os("APPIMAGE") {
        if let Some(p) = Path::new(&a).parent() {
            return p.to_path_buf();
        }
    }
    if cfg!(debug_assertions) {
        return Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    }
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

/// Die Datei des laufenden Launchers (portable .exe bzw. .AppImage). Aus den
/// Quellen oder zum Testen gestartet gibt es keine – dann kein Selbst-Update.
fn eigene_datei() -> Option<PathBuf> {
    if std::env::var_os("LAUNCHER_ROOT").is_some() || cfg!(debug_assertions) {
        return None;
    }
    if cfg!(target_os = "windows") {
        return std::env::current_exe().ok();
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

fn programm_ordner(app: &App, p: &Programm) -> PathBuf {
    app.wurzel.join("Programme").join(&p.ordner)
}

fn daten_ordner(app: &App, p: &Programm) -> PathBuf {
    app.wurzel.join("Daten").join(&p.daten)
}

/// Alte Launcher-Datei nach dem Update löschen. Sie ist eventuell noch kurz
/// gesperrt, bis sich der alte Prozess beendet hat – darum ein paar Versuche.
/// Gelöscht wird nur eine Tool-Launcher-Datei im selben Ordner.
fn alte_version_loeschen(alt: &Path) -> Option<String> {
    let name = alt.file_name()?.to_string_lossy().to_string();
    let muster = Regex::new(r"(?i)^Tool-Launcher.*\.(exe|AppImage)$").unwrap();
    if !muster.is_match(&name) {
        return None;
    }
    if let Some(eigene) = eigene_datei() {
        if eigene == alt || eigene.parent() != alt.parent() {
            return None;
        }
    }
    for _ in 0..30 {
        if !alt.exists() || fs::remove_file(alt).is_ok() {
            return Some(name);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    None
}

// --- Versionen ---------------------------------------------------------------

/// Versionsnummern vergleichen: 1.10.0 ist neuer als 1.9.2.
fn ist_neuer(neu: &str, alt: &str) -> bool {
    let teile = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split('.').map(|t| t.parse().unwrap_or(0)).collect() };
    let (a, b) = (teile(neu), teile(alt));
    for i in 0..3 {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[derive(Serialize, Deserialize, Clone)]
struct Installiert {
    version: String,
    datei: String,
}

fn installiert(app: &App, p: &Programm) -> Option<Installiert> {
    let ordner = programm_ordner(app, p);
    let v: Installiert = serde_json::from_str(&fs::read_to_string(ordner.join("version.json")).ok()?).ok()?;
    ordner.join(&v.datei).exists().then_some(v)
}

fn relativ(app: &App, p: &Path) -> String {
    p.strip_prefix(&app.wurzel).unwrap_or(p).to_string_lossy().to_string()
}

/// Liegt ein Programm schon neben dem Launcher (einzeln heruntergeladen) oder
/// in einem eigenen Ordner daneben (z. B. Paletten-Packschema/ auf dem Stick),
/// wird es übernommen: nach Programme/<Ordner>/ verschoben statt neu geladen.
/// Gibt es mehrere, zählt die höchste Version. Nur wenn noch nichts installiert ist.
fn vorhandene_uebernehmen(app: &App, p: &Programm) -> Option<Value> {
    if installiert(app, p).is_some() {
        return None;
    }
    let muster = Regex::new(p.dateien.get(SYSTEM)?).ok()?;
    let version_re = Regex::new(r"(\d+\.\d+\.\d+)").unwrap();
    let mut bester: Option<(PathBuf, String, String)> = None;
    for ordner in [app.wurzel.clone(), app.wurzel.join(&p.ordner)] {
        let Ok(liste) = fs::read_dir(&ordner) else { continue };
        for e in liste.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !muster.is_match(&name) {
                continue;
            }
            let version = version_re.captures(&name).map(|c| c[1].to_string()).unwrap_or_else(|| "0.0.0".into());
            if bester.as_ref().map(|b| ist_neuer(&version, &b.2)).unwrap_or(true) {
                bester = Some((e.path(), name, version));
            }
        }
    }
    let (pfad, name, version) = bester?;
    let ziel_ordner = programm_ordner(app, p);
    let ziel = ziel_ordner.join(&name);
    let ergebnis = (|| -> std::io::Result<()> {
        fs::create_dir_all(&ziel_ordner)?;
        if fs::rename(&pfad, &ziel).is_err() {
            // Anderes Laufwerk o. Ä.: kopieren, vergleichen, dann löschen
            fs::copy(&pfad, &ziel)?;
            if fs::metadata(&ziel)?.len() != fs::metadata(&pfad)?.len() {
                return Err(std::io::Error::other("Kopie unvollständig"));
            }
            fs::remove_file(&pfad)?;
        }
        ausfuehrbar(&ziel);
        fs::write(ziel_ordner.join("version.json"), serde_json::to_string_pretty(&Installiert { version: version.clone(), datei: name.clone() })?)?;
        Ok(())
    })();
    Some(match ergebnis {
        Ok(()) => json!({ "name": p.name, "version": version, "von": relativ(app, &pfad) }),
        // Etwa unter Windows, wenn das Programm gerade läuft
        Err(_) => json!({ "name": p.name, "fehler": format!("{} ließ sich nicht übernehmen (läuft es gerade?)", relativ(app, &pfad)) }),
    })
}

#[cfg(unix)]
fn ausfuehrbar(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
}
#[cfg(not(unix))]
fn ausfuehrbar(_p: &Path) {}

// --- Netz ------------------------------------------------------------------

/// HTTP-Client mit Zeitgrenze. Der Proxy kommt aus dem System (Umgebung bzw.
/// Windows-Einstellungen) – im Firmennetz hing der Launcher früher ohne.
fn client(gesamt: Option<Duration>) -> reqwest::Client {
    let mut b = reqwest::Client::builder().user_agent("Tool-Launcher").connect_timeout(Duration::from_secs(8));
    if let Some(t) = gesamt {
        b = b.timeout(t);
    }
    b.build().expect("HTTP-Client")
}

async fn hole_text(url: &str) -> Result<String, String> {
    let antwort = client(Some(Duration::from_secs(8))).get(url).header("Accept", "application/vnd.github+json").send().await.map_err(|_| "offline".to_string())?;
    if !antwort.status().is_success() {
        return Err(format!("HTTP {}", antwort.status().as_u16()));
    }
    antwort.text().await.map_err(|_| "offline".to_string())
}

/// Zeichen-Entitäten aus XML/HTML auflösen.
fn entitaeten(s: &str) -> String {
    let hex = Regex::new(r"(?i)&#x([0-9a-f]+);").unwrap();
    let dez = Regex::new(r"&#(\d+);").unwrap();
    let s = hex.replace_all(s, |c: &regex::Captures| u32::from_str_radix(&c[1], 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default());
    let s = dez.replace_all(&s, |c: &regex::Captures| c[1].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default());
    s.replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Release-Text aus dem Feed (HTML) als lesbarer Text.
fn html_zu_text(html: &str) -> String {
    let s = Regex::new(r"(?i)<li>").unwrap().replace_all(html, "- ");
    let s = Regex::new(r"(?i)<br\s*/?>\s*").unwrap().replace_all(&s, " ");
    let s = Regex::new(r"(?i)</(li|p|h\d|ul|ol)>").unwrap().replace_all(&s, "\n");
    let s = Regex::new(r"(?i)<h\d[^>]*>").unwrap().replace_all(&s, "\n");
    let s = Regex::new(r"<[^>]+>").unwrap().replace_all(&s, "");
    let s = entitaeten(&s);
    let s = Regex::new(r"[ \t]+\n").unwrap().replace_all(&s, "\n");
    Regex::new(r"\n{3,}").unwrap().replace_all(&s, "\n\n").trim().to_string()
}

struct FeedEintrag {
    tag: String,
    version: String,
    text: String,
    seite: String,
}

/// Neuestes Release mit dieser Kennung aus dem Atom-Feed des Repos. Der Feed
/// liegt auf github.com und zählt nicht gegen die 60 API-Abfragen pro Stunde,
/// die sich in der Firma alle Rechner teilen. Er zeigt nur die letzten 10 Releases.
fn aus_feed(xml: &str, repo: &str, kennung: &str) -> Option<FeedEintrag> {
    let link = Regex::new(r#"/releases/tag/([^"]+)""#).unwrap();
    let inhalt = Regex::new(r"(?s)<content[^>]*>(.*?)</content>").unwrap();
    let mut bester: Option<FeedEintrag> = None;
    for eintrag in xml.split("<entry>").skip(1) {
        let Some(l) = link.captures(eintrag) else { continue };
        let tag = l[1].replace("%2F", "/");
        let Some(version) = tag.strip_prefix(kennung) else { continue };
        if bester.as_ref().map(|b| !ist_neuer(version, &b.version)).unwrap_or(false) {
            continue;
        }
        let text = inhalt.captures(eintrag).map(|c| html_zu_text(&entitaeten(&c[1]))).unwrap_or_default();
        bester = Some(FeedEintrag { version: version.to_string(), seite: format!("https://github.com/{repo}/releases/tag/{tag}"), tag, text });
    }
    bester
}

/// Rückfall über die GitHub-API (zählt gegen das Limit).
async fn aus_api(repo: &str, muster: Option<&str>, kennung: &str) -> Option<Neueste> {
    let text = hole_text(&format!("https://api.github.com/repos/{repo}/releases?per_page=50")).await.ok()?;
    let liste: Vec<Value> = serde_json::from_str(&text).ok()?;
    let mut bester: Option<(&Value, String)> = None;
    for r in &liste {
        let tag = r["tag_name"].as_str().unwrap_or("");
        if r["draft"].as_bool().unwrap_or(false) || r["prerelease"].as_bool().unwrap_or(false) || !tag.starts_with(kennung) {
            continue;
        }
        let v = tag[kennung.len()..].to_string();
        if bester.as_ref().map(|b| ist_neuer(&v, &b.1)).unwrap_or(true) {
            bester = Some((r, v));
        }
    }
    let (r, version) = bester?;
    let re = muster.and_then(|m| Regex::new(m).ok());
    let datei = re.and_then(|re| {
        r["assets"].as_array()?.iter().find(|a| re.is_match(a["name"].as_str().unwrap_or(""))).map(|a| Datei {
            name: a["name"].as_str().unwrap_or("").to_string(),
            groesse: a["size"].as_u64(),
            url: a["browser_download_url"].as_str().unwrap_or("").to_string(),
        })
    });
    Some(Neueste { version, text: r["body"].as_str().unwrap_or("").to_string(), seite: r["html_url"].as_str().unwrap_or("").to_string(), datei })
}

/// Neueste Version eines Programms samt der Datei für dieses System. In einem
/// Sammel-Repo zählen nur Releases, deren Tag mit `kennung` beginnt.
/// `vorlage`: Dateiname mit {version} – daraus ergibt sich die Download-Adresse ohne API.
async fn neueste_version(repo: &str, vorlage: Option<&str>, muster: Option<&str>, kennung: &str) -> Result<Neueste, String> {
    let xml = hole_text(&format!("https://github.com/{repo}/releases.atom")).await.map_err(|_| "offline".to_string())?;
    let feed = aus_feed(&xml, repo, kennung);
    if let (Some(f), Some(v)) = (&feed, vorlage) {
        let name = v.replace("{version}", &f.version);
        return Ok(Neueste {
            version: f.version.clone(),
            text: f.text.clone(),
            seite: f.seite.clone(),
            datei: Some(Datei { url: format!("https://github.com/{repo}/releases/download/{}/{name}", f.tag), name, groesse: None }),
        });
    }
    // Nicht im Feed (älter als die letzten 10 Releases) oder keine Vorlage
    if let Some(api) = aus_api(repo, muster, kennung).await {
        return Ok(api);
    }
    feed.map(|f| Neueste { version: f.version, text: f.text, seite: f.seite, datei: None }).ok_or_else(|| "HTTP 404".into())
}

// --- Symbole ---------------------------------------------------------------

/// Symbol eines Programms als data:-URL. Erst das mitgelieferte, sonst aus dem
/// Repo – so bringen neue Programme in apps.json ihr Symbol ohne neuen Launcher mit.
async fn symbol(app: &App, p: &Programm) -> Option<String> {
    let icon = p.icon.clone()?;
    if !Regex::new(r"^[\w/.-]+\.png$").unwrap().is_match(&icon) || icon.contains("..") {
        return None;
    }
    if let Some(s) = app.zustand.lock().unwrap().symbole.get(&icon) {
        return s.clone();
    }
    let mut daten = eingebettet::SYMBOLE.iter().find(|(n, _)| *n == icon).map(|(_, d)| d.to_vec());
    if daten.is_none() {
        let url = format!("https://raw.githubusercontent.com/{LAUNCHER_REPO}/main/{icon}");
        if let Ok(a) = client(Some(Duration::from_secs(8))).get(url).send().await {
            if a.status().is_success() {
                daten = a.bytes().await.ok().map(|b| b.to_vec());
            }
        }
    }
    let url = daten.map(|d| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(d)));
    app.zustand.lock().unwrap().symbole.insert(icon, url.clone());
    url
}

async fn fuer_anzeige(app: &App, p: &Programm) -> Value {
    json!({
        "id": p.id, "name": p.name, "beschreibung": p.beschreibung,
        "installiert": installiert(app, p), "symbol": symbol(app, p).await,
    })
}

// --- Befehle für die Oberfläche ------------------------------------------------

/// Erst die mitgelieferte Liste – das Fenster ist sofort benutzbar, auch wenn
/// das Netz langsam ist. Die aktuelle Liste kommt danach (liste_online).
#[tauri::command]
async fn liste(handle: tauri::AppHandle, app: State<'_, App>) -> Result<Value, ()> {
    let programme = serde_json::from_str::<Liste>(eingebettet::APPS_JSON).map(|l| l.programme).unwrap_or_default();
    let uebernommen: Vec<Value> = programme.iter().filter_map(|p| vorhandene_uebernehmen(&app, p)).collect();
    app.zustand.lock().unwrap().programme = programme.clone();
    let mut anzeige = Vec::new();
    for p in &programme {
        anzeige.push(fuer_anzeige(&app, p).await);
    }
    let aktualisiert = app.alter_launcher.as_ref().map(|alt| {
        json!({ "von": alt.file_name().map(|n| n.to_string_lossy().to_string()), "geloescht": app.alte_geloescht.lock().unwrap().clone() })
    });
    Ok(json!({
        "uebernommen": uebernommen,
        "wurzel": app.wurzel.to_string_lossy(),
        "version": handle.package_info().version.to_string(),
        "aktualisiert": aktualisiert,
        "programme": anzeige,
    }))
}

#[tauri::command]
async fn liste_online(app: State<'_, App>) -> Result<Option<Vec<Value>>, ()> {
    let Ok(text) = hole_text(APPS_URL).await else { return Ok(None) };
    let Ok(liste) = serde_json::from_str::<Liste>(&text) else { return Ok(None) };
    // Programme, die nur die Online-Liste kennt, ebenfalls übernehmen
    for p in &liste.programme {
        vorhandene_uebernehmen(&app, p);
    }
    app.zustand.lock().unwrap().programme = liste.programme.clone();
    let mut anzeige = Vec::new();
    for p in &liste.programme {
        anzeige.push(fuer_anzeige(&app, p).await);
    }
    Ok(Some(anzeige))
}

#[tauri::command]
async fn pruefen(handle: tauri::AppHandle, app: State<'_, App>) -> Result<Value, ()> {
    let programme = app.zustand.lock().unwrap().programme.clone();
    // Alle Programme gleichzeitig prüfen statt nacheinander
    let abfragen = programme.iter().map(|p| async move {
        let kennung = p.tag.clone().unwrap_or_else(|| "v".into());
        (p.id.clone(), neueste_version(&p.repo, p.download.get(SYSTEM).map(String::as_str), p.dateien.get(SYSTEM).map(String::as_str), &kennung).await)
    });
    let ergebnisse = futures_util::future::join_all(abfragen).await;
    let mut liste = serde_json::Map::new();
    let mut offline = false;
    for (id, r) in ergebnisse {
        match r {
            Ok(n) => {
                liste.insert(id.clone(), json!({ "version": n.version, "text": n.text, "seite": n.seite, "hatDatei": n.datei.is_some() }));
                app.zustand.lock().unwrap().neueste.insert(id, n);
            }
            // 404: Es gibt noch kein Release – das ist keine Verbindungsstörung.
            Err(e) => offline |= !e.contains("404"),
        }
    }
    let win = cfg!(target_os = "windows");
    let mut launcher = Value::Null;
    if let Ok(n) = neueste_version(
        LAUNCHER_REPO,
        Some(if win { "Tool-Launcher-{version}.exe" } else { "Tool-Launcher-{version}.AppImage" }),
        Some(if win { r"^Tool-Launcher-.*\.exe$" } else { r"^Tool-Launcher-.*\.AppImage$" }),
        "v",
    )
    .await
    {
        if ist_neuer(&n.version, &handle.package_info().version.to_string()) {
            launcher = json!({ "version": n.version, "seite": n.seite, "hatDatei": n.datei.is_some(), "automatisch": n.datei.is_some() && eigene_datei().is_some() });
            app.zustand.lock().unwrap().neueste.insert("launcher".into(), n);
        }
    }
    Ok(json!({ "programme": liste, "launcher": launcher, "offline": offline }))
}

/// Lädt `datei` nach `ziel`: erst als .part, Größe prüfen, dann umbenennen.
/// Kommt 30 Sekunden lang nichts an, wird abgebrochen – statt ewig zu hängen.
async fn herunterladen(handle: &tauri::AppHandle, id: &str, datei: &Datei, ziel: &Path) -> Result<(), String> {
    let part = ziel.with_file_name(format!("{}.part", ziel.file_name().unwrap().to_string_lossy()));
    let haengt = "Der Download kommt nicht voran – Verbindung prüfen und noch einmal versuchen.".to_string();
    let antwort = tokio::time::timeout(Duration::from_secs(30), client(None).get(&datei.url).send())
        .await
        .map_err(|_| haengt.clone())?
        .map_err(|e| format!("Download fehlgeschlagen ({e})"))?;
    if !antwort.status().is_success() {
        return Err(format!("Download fehlgeschlagen (HTTP {})", antwort.status().as_u16()));
    }
    let gesamt = datei.groesse.or(antwort.content_length()).unwrap_or(0);
    let mut aus = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
    let mut strom = antwort.bytes_stream();
    let mut geladen: u64 = 0;
    let mut zuletzt = 0.0_f64;
    let ergebnis: Result<(), String> = async {
        loop {
            let stueck = tokio::time::timeout(Duration::from_secs(30), strom.next()).await.map_err(|_| haengt.clone())?;
            let Some(stueck) = stueck else { break };
            let stueck = stueck.map_err(|e| format!("Download abgebrochen ({e})"))?;
            aus.write_all(&stueck).await.map_err(|e| e.to_string())?;
            geladen += stueck.len() as u64;
            let anteil = if gesamt > 0 { geladen as f64 / gesamt as f64 } else { 0.0 };
            if anteil - zuletzt >= 0.01 || anteil >= 1.0 {
                zuletzt = anteil;
                let _ = handle.emit("fortschritt", json!({ "id": id, "anteil": anteil }));
            }
        }
        aus.flush().await.map_err(|e| e.to_string())
    }
    .await;
    drop(aus);
    if let Err(e) = ergebnis {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    if gesamt > 0 && fs::metadata(&part).map(|m| m.len()).unwrap_or(0) != gesamt {
        let _ = fs::remove_file(&part);
        return Err("Die Datei ist unvollständig angekommen – bitte noch einmal versuchen.".into());
    }
    fs::rename(&part, ziel).map_err(|e| e.to_string())?;
    ausfuehrbar(ziel);
    Ok(())
}

#[tauri::command]
async fn installieren(handle: tauri::AppHandle, app: State<'_, App>, id: String) -> Result<Value, ()> {
    let (p, n) = {
        let z = app.zustand.lock().unwrap();
        (z.programme.iter().find(|p| p.id == id).cloned(), z.neueste.get(&id).cloned())
    };
    let (Some(p), Some(Neueste { version, datei: Some(datei), .. })) = (p, n) else {
        return Ok(json!({ "ok": false, "fehler": "Für dieses System gibt es keine Datei." }));
    };
    let ordner = programm_ordner(&app, &p);
    let ergebnis = async {
        fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
        herunterladen(&handle, &id, &datei, &ordner.join(&datei.name)).await?;
        // Erst jetzt, wo die neue Version sicher da ist, die alte entfernen.
        let alt = installiert(&app, &p);
        fs::write(ordner.join("version.json"), serde_json::to_string_pretty(&Installiert { version, datei: datei.name.clone() }).unwrap()).map_err(|e| e.to_string())?;
        if let Some(alt) = alt.filter(|a| a.datei != datei.name) {
            let _ = fs::remove_file(ordner.join(alt.datei));
        }
        Ok::<_, String>(())
    }
    .await;
    Ok(match ergebnis {
        Ok(()) => json!({ "ok": true, "installiert": installiert(&app, &p) }),
        Err(e) => json!({ "ok": false, "fehler": e }),
    })
}

/// Programm unabhängig vom Launcher starten. Unter Linux erben Kindprozesse
/// sonst offene Dateien aus dem eingehängten AppImage des Launchers – dann
/// bleibt der alte Launcher hängen, bis das Programm beendet ist. Darum über
/// bash alle Dateinummern außer 0–2 schließen (die einfache sh kann nur bis 9).
fn loslassen(datei: &Path, args: &[String], cwd: &Path, env: &[(&str, &Path)], entfernen: &[&str]) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    #[cfg(unix)]
    let mut cmd = if Path::new("/bin/bash").exists() {
        let mut c = Command::new("/bin/bash");
        c.arg("-c").arg(r#"for f in /proc/$$/fd/*; do n=${f##*/}; [ "$n" -gt 2 ] 2>/dev/null && eval "exec $n>&-"; done; exec "$0" "$@""#).arg(datei).args(args);
        c
    } else {
        let mut c = Command::new(datei);
        c.args(args);
        c
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut c = Command::new(datei);
        c.args(args);
        c
    };
    cmd.current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for k in entfernen {
        cmd.env_remove(k);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    cmd.spawn().map(|_| ())
}

/// Selbst-Update: neue Version neben die alte laden, starten (mit dem Pfad der
/// alten Datei, damit sie aufräumt) und diesen Launcher beenden. Aus den
/// Quellen gestartet wird nur heruntergeladen.
#[tauri::command]
async fn selbst_update(handle: tauri::AppHandle, app: State<'_, App>) -> Result<Value, ()> {
    let n = app.zustand.lock().unwrap().neueste.get("launcher").cloned();
    let Some(Neueste { datei: Some(datei), .. }) = n else {
        return Ok(json!({ "ok": false, "fehler": "Für dieses System gibt es keine Datei." }));
    };
    let eigene = eigene_datei();
    let ordner = eigene.as_ref().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_else(|| app.wurzel.clone());
    let ziel = ordner.join(&datei.name);
    if let Err(e) = herunterladen(&handle, "launcher", &datei, &ziel).await {
        return Ok(json!({ "ok": false, "fehler": e }));
    }
    let Some(eigene) = eigene else {
        return Ok(json!({ "ok": true, "datei": datei.name, "neustart": false }));
    };
    // Umgebungsvariablen der alten Datei nicht weitergeben – die neue setzt eigene.
    let entfernen = ["PORTABLE_EXECUTABLE_FILE", "PORTABLE_EXECUTABLE_DIR", "PORTABLE_EXECUTABLE_APP_FILENAME", "APPIMAGE", "APPDIR", "ARGV0", "OWD"];
    if let Err(e) = loslassen(&ziel, &[format!("{ALT_ARG}{}", eigene.to_string_lossy())], &ordner, &[], &entfernen) {
        return Ok(json!({ "ok": false, "fehler": format!("Die neue Version ließ sich nicht starten: {e}") }));
    }
    let h = handle.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        h.exit(0);
    });
    Ok(json!({ "ok": true, "datei": datei.name, "neustart": true }))
}

/// Einmaliger Umzug alter Daten (die neben dem Programm selbst lagen) in den
/// Daten-Ordner des Launchers. Kopieren, Inhalt vergleichen, erst dann das
/// Original löschen. Was am Ziel schon existiert, wird nie überschrieben.
fn daten_umziehen(app: &App, p: &Programm) -> Value {
    let mut verschoben = Vec::new();
    let mut uebersprungen = Vec::new();
    if let Some(u) = &p.umzug {
        let ziel = daten_ordner(app, p);
        for von in &u.von {
            let quelle = app.wurzel.join(von);
            if quelle == ziel {
                continue;
            }
            let mut etwas = false;
            for name in &u.dateien {
                let alt = quelle.join(name);
                let Ok(inhalt) = fs::read(&alt) else { continue };
                // Gleiche Dateinamen bei mehreren Programmen (einstellungen.json):
                // nur übernehmen, was am Inhalt erkennbar zu diesem Programm gehört.
                if let Some(m) = u.merkmal.get(name) {
                    if !String::from_utf8_lossy(&inhalt).contains(&format!("\"{m}\"")) {
                        continue;
                    }
                }
                let neu = ziel.join(name);
                let anzeige = format!("{von}/{name}");
                if neu.exists() {
                    uebersprungen.push(anzeige);
                    continue;
                }
                let ok = fs::create_dir_all(&ziel).is_ok() && fs::write(&neu, &inhalt).is_ok() && fs::read(&neu).map(|n| n == inhalt).unwrap_or(false);
                if !ok {
                    let _ = fs::remove_file(&neu);
                    uebersprungen.push(anzeige);
                    continue;
                }
                let _ = fs::remove_file(&alt);
                verschoben.push(anzeige);
                etwas = true;
            }
            if etwas {
                let _ = fs::write(
                    quelle.join(format!("UMGEZOGEN-{}.txt", p.daten)),
                    format!("Die Daten von {} liegen jetzt in Daten/{}/ neben dem Tool Launcher.\nDas Programm bitte über den Launcher starten, dann sind alle Schemas da.\n", p.name, p.daten),
                );
            }
        }
    }
    json!({ "verschoben": verschoben, "uebersprungen": uebersprungen })
}

#[tauri::command]
async fn starten(app: State<'_, App>, id: String) -> Result<Value, ()> {
    let p = app.zustand.lock().unwrap().programme.iter().find(|p| p.id == id).cloned();
    let Some((p, v)) = p.and_then(|p| installiert(&app, &p).map(|v| (p, v))) else {
        return Ok(json!({ "ok": false, "fehler": "Das Programm ist nicht installiert." }));
    };
    let umzug = daten_umziehen(&app, &p);
    let daten = daten_ordner(&app, &p);
    let _ = fs::create_dir_all(&daten);
    let ordner = programm_ordner(&app, &p);
    Ok(match loslassen(&ordner.join(&v.datei), &[], &ordner, &[("LAUNCHER_DATEN_DIR", &daten)], &[]) {
        Ok(()) => json!({ "ok": true, "umzug": umzug }),
        Err(e) => json!({ "ok": false, "fehler": e.to_string(), "umzug": umzug }),
    })
}

#[tauri::command]
fn ordner_oeffnen(app: State<'_, App>, id: String) {
    let p = app.zustand.lock().unwrap().programme.iter().find(|p| p.id == id).cloned();
    let ordner = p.map(|p| daten_ordner(&app, &p)).unwrap_or_else(|| app.wurzel.clone());
    let _ = fs::create_dir_all(&ordner);
    let _ = open::that_detached(ordner);
}

#[tauri::command]
fn link_oeffnen(url: String) {
    if url.starts_with("https://") {
        let _ = open::that_detached(url);
    }
}

// --- Testschnittstelle (nur in der Entwicklerfassung) ----------------------
// TEST_SKRIPT=<datei.js>: wird nach dem Laden im Fenster ausgeführt.

#[cfg(debug_assertions)]
#[tauri::command]
fn test_log(text: String) {
    println!("TEST {text}");
}

#[cfg(debug_assertions)]
#[tauri::command]
fn test_bild(name: String) {
    if let Some(dir) = std::env::var_os("TEST_AUSGABE") {
        let _ = std::process::Command::new("import").args(["-window", "root"]).arg(PathBuf::from(dir).join(format!("{name}.png"))).status();
    }
}

#[cfg(debug_assertions)]
#[tauri::command]
fn test_ende(app: tauri::AppHandle, code: i32) {
    app.exit(code);
}

fn main() {
    let alter_launcher = std::env::args().find_map(|a| a.strip_prefix(ALT_ARG).map(PathBuf::from));
    let alte_geloescht = alter_launcher.as_deref().and_then(alte_version_loeschen);
    let zustand = App { zustand: Mutex::new(Zustand::default()), wurzel: wurzel_ordner(), alter_launcher, alte_geloescht: Mutex::new(alte_geloescht) };

    let builder = tauri::Builder::default().manage(zustand);
    #[cfg(debug_assertions)]
    let builder = builder
        .invoke_handler(tauri::generate_handler![liste, liste_online, pruefen, installieren, selbst_update, starten, ordner_oeffnen, link_oeffnen, test_log, test_bild, test_ende])
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                if let Some(skript) = std::env::var_os("TEST_SKRIPT").and_then(|p| fs::read_to_string(p).ok()) {
                    let _ = webview.eval(skript);
                }
            }
        });
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![liste, liste_online, pruefen, installieren, selbst_update, starten, ordner_oeffnen, link_oeffnen]);
    builder.run(tauri::generate_context!()).expect("Programm konnte nicht starten");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versionen() {
        assert!(ist_neuer("1.10.0", "1.9.2"));
        assert!(ist_neuer("v2.0.0", "1.3.0"));
        assert!(!ist_neuer("1.3.0", "1.3.0"));
    }

    #[test]
    fn feed() {
        let xml = r#"<feed><entry><link href="https://github.com/Marcel1853/tool-releases/releases/tag/paletten-packschema-v1.8.0"/>
            <content type="html">&lt;h3&gt;Behoben&lt;/h3&gt;&lt;ul&gt;&lt;li&gt;Hochkant &amp;amp; Säulen&lt;/li&gt;&lt;/ul&gt;</content></entry>
            <entry><link href="https://github.com/Marcel1853/tool-releases/releases/tag/paletten-packschema-v1.7.0"/></entry></feed>"#;
        let f = aus_feed(xml, "Marcel1853/tool-releases", "paletten-packschema-v").unwrap();
        assert_eq!(f.version, "1.8.0");
        assert_eq!(f.text, "Behoben\n- Hochkant & Säulen");
    }

    #[test]
    fn eingebettete_liste() {
        let l: Liste = serde_json::from_str(eingebettet::APPS_JSON).unwrap();
        assert!(l.programme.iter().any(|p| p.id == "paletten-packschema"));
        for p in &l.programme {
            if let Some(i) = &p.icon {
                assert!(eingebettet::SYMBOLE.iter().any(|(n, _)| n == i), "Symbol fehlt: {i}");
            }
        }
    }
}
