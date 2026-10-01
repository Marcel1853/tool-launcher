// Hauptprozess des Tool Launchers: kennt die Programme, lädt neue Versionen
// herunter und startet sie. Die Oberfläche läuft ohne Node im Renderer.
//
// Aufbau neben dem Launcher (etwa auf dem USB-Stick):
//   Programme/<Ordner>/   das Programm selbst + version.json (wird ersetzt)
//   Daten/<Ordner>/       dessen Daten (bleiben immer)

const { app, BrowserWindow, Menu, shell, ipcMain } = require("electron");
const path = require("path");
const fs = require("fs");
const { spawn } = require("child_process");

const LAUNCHER_REPO = "Marcel1853/tool-launcher";
// Die Programmliste kommt aus dem öffentlichen Repo, damit neue Programme ohne
// neuen Launcher dazukommen. Ohne Internet gilt die mitgelieferte Liste.
const APPS_URL = `https://raw.githubusercontent.com/${LAUNCHER_REPO}/main/apps.json`;

// --- Ordner ----------------------------------------------------------------

function wurzelOrdner() {
    // Zum Testen: eigener Ordner statt neben dem Programm.
    if (process.env.LAUNCHER_ROOT) return process.env.LAUNCHER_ROOT;
    // Windows, portable Fassung: der Ordner, in dem die .exe liegt.
    if (process.env.PORTABLE_EXECUTABLE_DIR) return process.env.PORTABLE_EXECUTABLE_DIR;
    // Linux, AppImage: der Ordner, in dem die .AppImage-Datei liegt.
    if (process.env.APPIMAGE) return path.dirname(process.env.APPIMAGE);
    // Aus den Quellen gestartet (npm start): der Projektordner.
    if (!app.isPackaged) return __dirname;
    return path.dirname(app.getPath("exe"));
}

const WURZEL = wurzelOrdner();

/** Die Datei des laufenden Launchers (portable .exe bzw. .AppImage). Aus den
 *  Quellen gestartet gibt es keine – dann kein Selbst-Update. */
function eigeneDatei() {
    if (process.env.LAUNCHER_ROOT) return null;
    return process.env.PORTABLE_EXECUTABLE_FILE || process.env.APPIMAGE || null;
}

// Nach einem Selbst-Update startet die neue Version mit dem Pfad der alten.
const ALT_ARG = "--alter-launcher=";
const alterLauncher = (process.argv.find(a => a.startsWith(ALT_ARG)) || "").slice(ALT_ARG.length) || null;

/**
 * Alte Launcher-Datei nach dem Update löschen. Sie ist eventuell noch kurz
 * gesperrt, bis sich der alte Prozess beendet hat – darum ein paar Versuche.
 * Gelöscht wird nur eine Tool-Launcher-Datei im selben Ordner.
 */
async function alteVersionLoeschen() {
    if (!alterLauncher) return null;
    const eigene = eigeneDatei();
    const alt = path.resolve(alterLauncher);
    if (!/^Tool-Launcher.*\.(exe|AppImage)$/i.test(path.basename(alt))) return null;
    if (eigene && (path.resolve(eigene) === alt || path.dirname(path.resolve(eigene)) !== path.dirname(alt))) return null;
    for (let i = 0; i < 30; i++) {
        try {
            if (!fs.existsSync(alt)) return path.basename(alt);
            fs.rmSync(alt);
            return path.basename(alt);
        } catch (e) {
            await new Promise(r => setTimeout(r, 500));
        }
    }
    return null;
}
let alteGeloescht = null;
const programmOrdner = p => path.join(WURZEL, "Programme", p.ordner);
const datenOrdner = p => path.join(WURZEL, "Daten", p.daten);

// --- Programmliste und Versionen ---------------------------------------------

let programme = [];

async function holeJson(url) {
    const antwort = await fetch(url, {
        headers: { Accept: "application/vnd.github+json", "User-Agent": "Tool-Launcher" },
        signal: AbortSignal.timeout(10000),
    });
    if (!antwort.ok) throw new Error(`HTTP ${antwort.status}`);
    return antwort.json();
}

async function ladeProgrammliste() {
    try {
        const liste = await holeJson(APPS_URL);
        if (Array.isArray(liste.programme)) return liste.programme;
    } catch (e) { /* ohne Internet: mitgelieferte Liste */ }
    return JSON.parse(fs.readFileSync(path.join(__dirname, "apps.json"), "utf-8")).programme;
}

/** Versionsnummern vergleichen: 1.10.0 ist neuer als 1.9.2. */
function istNeuer(neu, alt) {
    const a = String(neu).replace(/^v/, "").split(".").map(Number);
    const b = String(alt || "0").replace(/^v/, "").split(".").map(Number);
    for (let i = 0; i < 3; i++) {
        if ((a[i] || 0) !== (b[i] || 0)) return (a[i] || 0) > (b[i] || 0);
    }
    return false;
}

function installiert(p) {
    try {
        const v = JSON.parse(fs.readFileSync(path.join(programmOrdner(p), "version.json"), "utf-8"));
        return fs.existsSync(path.join(programmOrdner(p), v.datei)) ? v : null;
    } catch (e) {
        return null;
    }
}

/** Zeichen-Entitäten aus XML/HTML auflösen. */
function entitaeten(s) {
    return String(s)
        .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
        .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(Number(d)))
        .replace(/&quot;/g, '"').replace(/&#39;|&apos;/g, "'")
        .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
}

/** Release-Text aus dem Feed (HTML) als lesbarer Text. */
function htmlZuText(html) {
    return entitaeten(String(html)
        .replace(/<li>/gi, "- ")
        .replace(/<br\s*\/?>\s*/gi, " ")
        .replace(/<\/(li|p|h\d|ul|ol)>/gi, "\n")
        .replace(/<h\d[^>]*>/gi, "\n")
        .replace(/<[^>]+>/g, ""))
        .replace(/[ \t]+\n/g, "\n").replace(/\n{3,}/g, "\n\n").trim();
}

/**
 * Neuestes Release mit dieser Kennung aus dem Atom-Feed des Repos. Der Feed
 * liegt auf github.com selbst und zählt nicht gegen die 60 Abfragen pro
 * Stunde, die die GitHub-API ohne Anmeldung erlaubt (in der Firma teilen sich
 * alle Rechner diesen Wert). Er zeigt allerdings nur die letzten 10 Releases.
 */
async function ausFeed(repo, kennung) {
    const antwort = await fetch(`https://github.com/${repo}/releases.atom`, {
        headers: { "User-Agent": "Tool-Launcher" },
        signal: AbortSignal.timeout(10000),
    });
    if (!antwort.ok) throw new Error(`HTTP ${antwort.status}`);
    const xml = await antwort.text();
    let bester = null;
    for (const eintrag of xml.split("<entry>").slice(1)) {
        const link = /\/releases\/tag\/([^"]+)"/.exec(eintrag);
        if (!link) continue;
        const tag = decodeURIComponent(link[1]);
        if (!tag.startsWith(kennung)) continue;
        const version = tag.slice(kennung.length);
        if (bester && !istNeuer(version, bester.version)) continue;
        const inhalt = /<content[^>]*>([\s\S]*?)<\/content>/.exec(eintrag);
        bester = {
            tag, version,
            text: inhalt ? htmlZuText(entitaeten(inhalt[1])) : "",
            seite: `https://github.com/${repo}/releases/tag/${encodeURIComponent(tag)}`,
        };
    }
    return bester;
}

/** Rückfall über die GitHub-API (zählt gegen das Limit). */
async function ausApi(repo, muster, kennung) {
    const liste = await holeJson(`https://api.github.com/repos/${repo}/releases?per_page=50`);
    let release = null, version = null;
    for (const r of liste) {
        if (r.draft || r.prerelease || !String(r.tag_name).startsWith(kennung)) continue;
        const v = String(r.tag_name).slice(kennung.length);
        if (!release || istNeuer(v, version)) { release = r; version = v; }
    }
    if (!release) return null;
    const re = muster ? new RegExp(muster) : null;
    const datei = re && (release.assets || []).find(a => re.test(a.name));
    return {
        version,
        text: release.body || "",
        seite: release.html_url,
        datei: datei ? { name: datei.name, groesse: datei.size, url: datei.browser_download_url } : null,
    };
}

/**
 * Neueste Version eines Programms samt der Datei für dieses Betriebssystem.
 * In einem Sammel-Repo zählen nur Releases, deren Tag mit `kennung` beginnt.
 * `vorlage`: Dateiname mit {version}, z. B. „Paletten-Packschema-{version}.exe“
 * – daraus ergibt sich die Download-Adresse ohne API.
 */
async function neuesteVersion(repo, vorlage, muster, kennung) {
    let feed;
    try {
        feed = await ausFeed(repo, kennung);
    } catch (e) {
        throw new Error("offline"); // github.com nicht erreichbar
    }
    if (feed && vorlage) {
        const name = vorlage.replace("{version}", feed.version);
        return {
            version: feed.version, text: feed.text, seite: feed.seite,
            datei: { name, groesse: null, url: `https://github.com/${repo}/releases/download/${encodeURIComponent(feed.tag)}/${encodeURIComponent(name)}` },
        };
    }
    // Nicht im Feed (älter als die letzten 10 Releases) oder keine Vorlage.
    try {
        const api = await ausApi(repo, muster, kennung);
        if (api) return api;
    } catch (e) { /* Limit erreicht – dann eben ohne */ }
    if (feed) return { version: feed.version, text: feed.text, seite: feed.seite, datei: null };
    throw new Error("HTTP 404"); // noch nichts veröffentlicht
}

const neueste = new Map(); // id → Ergebnis von neuesteVersion

/**
 * Symbol eines Programms als data:-URL (die Oberfläche darf nur eigene
 * Dateien laden). Erst im Launcher mitgeliefert, sonst aus dem Repo – so
 * bringen neue Programme in apps.json ihr Symbol ohne neuen Launcher mit.
 */
const symbole = new Map();
async function symbol(p) {
    if (!p.icon || !/^[\w\/.-]+\.png$/.test(p.icon) || p.icon.includes("..")) return null;
    if (symbole.has(p.icon)) return symbole.get(p.icon);
    let daten = null;
    try {
        daten = fs.readFileSync(path.join(__dirname, p.icon));
    } catch (e) {
        try {
            const antwort = await fetch(`https://raw.githubusercontent.com/${LAUNCHER_REPO}/main/${p.icon}`, { signal: AbortSignal.timeout(8000) });
            if (antwort.ok) daten = Buffer.from(await antwort.arrayBuffer());
        } catch (e2) { /* ohne Symbol */ }
    }
    const url = daten ? `data:image/png;base64,${daten.toString("base64")}` : null;
    symbole.set(p.icon, url);
    return url;
}

ipcMain.handle("launcher:liste", async () => {
    programme = await ladeProgrammliste();
    return {
        wurzel: WURZEL,
        version: app.getVersion(),
        // Gerade per Selbst-Update gestartet? Dann den Namen der alten Datei.
        aktualisiert: alterLauncher ? { von: path.basename(alterLauncher), geloescht: alteGeloescht } : null,
        programme: await Promise.all(programme.map(async p => ({
            id: p.id, name: p.name, beschreibung: p.beschreibung, installiert: installiert(p), symbol: await symbol(p),
        }))),
    };
});

ipcMain.handle("launcher:pruefen", async () => {
    const ergebnis = { programme: {}, launcher: null, offline: false };
    for (const p of programme) {
        try {
            const n = await neuesteVersion(p.repo, p.download && p.download[process.platform], p.dateien && p.dateien[process.platform], p.tag || "v");
            neueste.set(p.id, n);
            ergebnis.programme[p.id] = { version: n.version, text: n.text, seite: n.seite, hatDatei: !!n.datei };
        } catch (e) {
            // 404: Es gibt noch kein Release – das ist keine Verbindungsstörung.
            if (!/HTTP 404/.test(e.message)) ergebnis.offline = true;
        }
    }
    try {
        const win = process.platform === "win32";
        const n = await neuesteVersion(LAUNCHER_REPO, win ? "Tool-Launcher-{version}.exe" : "Tool-Launcher-{version}.AppImage",
            win ? "^Tool-Launcher-.*\\.exe$" : "^Tool-Launcher-.*\\.AppImage$", "v");
        if (istNeuer(n.version, app.getVersion())) {
            neueste.set("launcher", n);
            ergebnis.launcher = { version: n.version, seite: n.seite, hatDatei: !!n.datei, automatisch: !!(n.datei && eigeneDatei()) };
        }
    } catch (e) { /* noch kein Release oder offline */ }
    return ergebnis;
});

// --- Herunterladen -----------------------------------------------------------

/** Lädt `datei` nach `ziel`: erst als .part, Größe prüfen, dann umbenennen. */
async function herunterladen(datei, ziel, fortschritt) {
    const part = ziel + ".part";
    const antwort = await fetch(datei.url, { headers: { "User-Agent": "Tool-Launcher" } });
    if (!antwort.ok || !antwort.body) throw new Error(`Download fehlgeschlagen (HTTP ${antwort.status})`);
    const gesamt = datei.groesse || Number(antwort.headers.get("content-length")) || 0;
    const aus = fs.createWriteStream(part);
    let geladen = 0;
    try {
        const leser = antwort.body.getReader();
        for (; ;) {
            const { done, value } = await leser.read();
            if (done) break;
            geladen += value.length;
            if (!aus.write(Buffer.from(value))) await new Promise(r => aus.once("drain", r));
            fortschritt(gesamt ? geladen / gesamt : 0);
        }
        await new Promise((ok, fehler) => aus.end(e => (e ? fehler(e) : ok())));
    } catch (e) {
        aus.destroy();
        fs.rmSync(part, { force: true });
        throw e;
    }
    if (gesamt && fs.statSync(part).size !== gesamt) {
        fs.rmSync(part, { force: true });
        throw new Error("Die Datei ist unvollständig angekommen – bitte noch einmal versuchen.");
    }
    fs.renameSync(part, ziel);
    if (process.platform !== "win32") fs.chmodSync(ziel, 0o755);
}

ipcMain.handle("launcher:installieren", async (ereignis, id) => {
    const p = programme.find(x => x.id === id);
    const n = neueste.get(id);
    if (!p || !n || !n.datei) return { ok: false, fehler: "Für dieses System gibt es keine Datei." };
    const ordner = programmOrdner(p);
    try {
        fs.mkdirSync(ordner, { recursive: true });
        const ziel = path.join(ordner, n.datei.name);
        await herunterladen(n.datei, ziel, anteil => ereignis.sender.send("launcher:fortschritt", id, anteil));
        // Erst jetzt, wo die neue Version sicher da ist, die alten entfernen.
        const alt = installiert(p);
        fs.writeFileSync(path.join(ordner, "version.json"), JSON.stringify({ version: n.version, datei: n.datei.name }, null, 2));
        if (alt && alt.datei !== n.datei.name) fs.rmSync(path.join(ordner, alt.datei), { force: true });
        return { ok: true, installiert: installiert(p) };
    } catch (e) {
        return { ok: false, fehler: e.message };
    }
});

/**
 * Selbst-Update: neue Version neben die alte laden, starten (mit dem Pfad der
 * alten Datei, damit sie aufräumt) und diesen Launcher beenden. Aus den
 * Quellen gestartet wird nur heruntergeladen.
 */
ipcMain.handle("launcher:selbstUpdate", async ereignis => {
    const n = neueste.get("launcher");
    if (!n || !n.datei) return { ok: false, fehler: "Für dieses System gibt es keine Datei." };
    const eigene = eigeneDatei();
    const ordner = eigene ? path.dirname(eigene) : WURZEL;
    try {
        const ziel = path.join(ordner, n.datei.name);
        await herunterladen(n.datei, ziel, anteil => ereignis.sender.send("launcher:fortschritt", "launcher", anteil));
        if (!eigene) return { ok: true, datei: n.datei.name, neustart: false };

        // Umgebungsvariablen der alten Datei nicht weitergeben – die neue setzt eigene.
        const env = { ...process.env };
        ["PORTABLE_EXECUTABLE_FILE", "PORTABLE_EXECUTABLE_DIR", "PORTABLE_EXECUTABLE_APP_FILENAME", "APPIMAGE", "APPDIR", "ARGV0", "OWD"].forEach(k => delete env[k]);
        const kind = spawn(ziel, [ALT_ARG + eigene], { cwd: ordner, env, detached: true, stdio: "ignore" });
        const fehler = await new Promise(fertig => {
            kind.once("spawn", () => fertig(null));
            kind.once("error", e => fertig(e));
        });
        if (fehler) return { ok: false, fehler: `Die neue Version ließ sich nicht starten: ${fehler.message}` };
        kind.unref();
        setTimeout(() => app.quit(), 300);
        return { ok: true, datei: n.datei.name, neustart: true };
    } catch (e) {
        return { ok: false, fehler: e.message };
    }
});

// --- Daten-Umzug ---------------------------------------------------------------

/**
 * Einmaliger Umzug alter Daten (die neben dem Programm selbst lagen) in den
 * Daten-Ordner des Launchers. Kopieren, Inhalt vergleichen, erst dann das
 * Original löschen. Was am Ziel schon existiert, wird nie überschrieben.
 */
function datenUmziehen(p) {
    const bericht = { verschoben: [], uebersprungen: [] };
    if (!p.umzug) return bericht;
    const ziel = datenOrdner(p);
    for (const von of p.umzug.von) {
        const quelle = path.join(WURZEL, von);
        if (path.resolve(quelle) === path.resolve(ziel)) continue;
        let verschoben = false;
        for (const name of p.umzug.dateien) {
            const alt = path.join(quelle, name);
            if (!fs.existsSync(alt)) continue;
            const neu = path.join(ziel, name);
            if (fs.existsSync(neu)) { bericht.uebersprungen.push(path.join(von, name)); continue; }
            fs.mkdirSync(ziel, { recursive: true });
            fs.copyFileSync(alt, neu);
            if (!fs.readFileSync(alt).equals(fs.readFileSync(neu))) {
                fs.rmSync(neu, { force: true });
                bericht.uebersprungen.push(path.join(von, name));
                continue;
            }
            fs.rmSync(alt);
            bericht.verschoben.push(path.join(von, name));
            verschoben = true;
        }
        if (verschoben) {
            fs.writeFileSync(path.join(quelle, "UMGEZOGEN.txt"),
                `Die Daten von ${p.name} liegen jetzt in Daten/${p.daten}/ neben dem Tool Launcher.\n` +
                `Das Programm bitte über den Launcher starten, dann sind alle Schemas da.\n`);
        }
    }
    return bericht;
}

ipcMain.handle("launcher:starten", async (_ereignis, id) => {
    const p = programme.find(x => x.id === id);
    const v = p && installiert(p);
    if (!v) return { ok: false, fehler: "Das Programm ist nicht installiert." };
    const umzug = datenUmziehen(p);
    fs.mkdirSync(datenOrdner(p), { recursive: true });
    try {
        const kind = spawn(path.join(programmOrdner(p), v.datei), [], {
            cwd: programmOrdner(p),
            env: { ...process.env, LAUNCHER_DATEN_DIR: datenOrdner(p) },
            detached: true,
            stdio: "ignore",
        });
        // Startfehler (z. B. fehlende Rechte) kommen erst nach dem Aufruf.
        const fehler = await new Promise(fertig => {
            kind.once("spawn", () => fertig(null));
            kind.once("error", e => fertig(e));
        });
        if (fehler) return { ok: false, fehler: fehler.message, umzug };
        kind.unref();
        return { ok: true, umzug };
    } catch (e) {
        return { ok: false, fehler: e.message, umzug };
    }
});

ipcMain.handle("launcher:ordner", (_ereignis, id) => {
    const p = programme.find(x => x.id === id);
    const ordner = p ? datenOrdner(p) : WURZEL;
    fs.mkdirSync(ordner, { recursive: true });
    return shell.openPath(ordner);
});

// --- Fenster -------------------------------------------------------------------

function fensterBauen() {
    const win = new BrowserWindow({
        width: 1040,
        height: 680,
        minWidth: 640,
        minHeight: 420,
        backgroundColor: "#151a23",
        icon: path.join(__dirname, "icon.png"),
        autoHideMenuBar: true,
        show: false,
        title: "Tool Launcher",
        webPreferences: {
            preload: path.join(__dirname, "preload.js"),
            contextIsolation: true,
            nodeIntegration: false,
            sandbox: true,
        },
    });
    win.once("ready-to-show", () => win.show());
    win.loadFile("index.html");
    // Links (Release-Seiten) im Standardbrowser öffnen.
    win.webContents.setWindowOpenHandler(({ url }) => {
        if (/^https:\/\//.test(url)) shell.openExternal(url);
        return { action: "deny" };
    });
}

app.whenReady().then(async () => {
    Menu.setApplicationMenu(null);
    alteGeloescht = await alteVersionLoeschen();
    fensterBauen();
});

app.on("window-all-closed", () => app.quit());
