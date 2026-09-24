/* Tool Launcher – Oberfläche */

(function () {
    "use strict";

    const $ = id => document.getElementById(id);
    let daten = null;        // { wurzel, version, programme: [...] }
    let neueste = {};        // id → { version, text, seite, hatDatei }
    let offline = false;
    let geprueft = false;   // Update-Suche schon gelaufen?
    const beschaeftigt = new Set();

    /** Text sicher in HTML einsetzen. */
    const esc = s => String(s).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

    function istNeuer(neu, alt) {
        const a = String(neu).split(".").map(Number), b = String(alt || "0").split(".").map(Number);
        for (let i = 0; i < 3; i++) if ((a[i] || 0) !== (b[i] || 0)) return (a[i] || 0) > (b[i] || 0);
        return false;
    }

    function setStatus(text) { $("status").textContent = text || ""; }

    function karte(p) {
        const n = neueste[p.id];
        const inst = p.installiert;
        const updateDa = inst && n && istNeuer(n.version, inst.version);
        const busy = beschaeftigt.has(p.id);
        const dis = busy ? "disabled" : "";

        const marke = updateDa ? `<span class="marke marke-update">Update verfügbar</span>`
            : inst && n ? `<span class="marke marke-aktuell">Aktuell</span>`
                : !inst ? `<span class="marke marke-fehlt">Nicht installiert</span>` : "";

        const neuesteText = n ? esc(n.version)
            : offline ? "keine Verbindung"
                : geprueft ? "noch keine" : "…";

        const knoepfe = [];
        if (inst) knoepfe.push(`<button class="btn" data-aktion="starten" data-id="${p.id}" ${dis}>Starten</button>`);
        if (!inst && n && n.hatDatei) knoepfe.push(`<button class="btn" data-aktion="installieren" data-id="${p.id}" ${dis}>Installieren</button>`);
        if (updateDa && n.hatDatei) knoepfe.push(`<button class="btn" data-aktion="installieren" data-id="${p.id}" ${dis}>Auf ${esc(n.version)} aktualisieren</button>`);
        if (inst) knoepfe.push(`<button class="btn btn-ghost" data-aktion="ordner" data-id="${p.id}">Daten-Ordner</button>`);

        const aenderungen = n && n.text && (updateDa || !inst)
            ? `<details class="aenderungen"><summary>Was ist neu in ${esc(n.version)}?</summary><pre>${esc(n.text)}</pre>
               <a href="${esc(n.seite)}" target="_blank" rel="noopener">Release-Seite öffnen</a></details>`
            : "";
        const keineDatei = n && !n.hatDatei ? `<p class="leise">Für dieses Betriebssystem gibt es keine Datei.</p>` : "";
        const symbol = p.symbol ? `<img class="symbol" src="${p.symbol}" alt="" />` : "";

        return `
            <section class="view${updateDa ? " hat-update" : ""}">
                <div class="view-head">
                    <div class="titel">${symbol}<h3>${esc(p.name)}</h3></div>
                    ${marke}
                </div>
                <p class="beschreibung">${esc(p.beschreibung || "")}</p>
                <dl class="versionen">
                    <div><dt>Installiert</dt><dd>${inst ? esc(inst.version) : "–"}</dd></div>
                    <div><dt>Neueste</dt><dd>${neuesteText}</dd></div>
                </dl>
                ${keineDatei}
                <div class="fortschritt" id="fs-${p.id}" hidden><div></div></div>
                <div class="knoepfe">${knoepfe.join("")}</div>
                ${aenderungen}
            </section>`;
    }

    function zeichnen() {
        const progs = daten.programme;
        const installiert = progs.filter(p => p.installiert).length;
        const updates = progs.filter(p => p.installiert && neueste[p.id] && istNeuer(neueste[p.id].version, p.installiert.version)).length;
        const stats = [
            ["Programme", progs.length, true],
            ["Installiert", installiert],
            ["Updates verfügbar", geprueft && !offline ? updates : "–"],
        ];
        $("stats").innerHTML = stats.map(([label, value, main]) =>
            `<div class="stat${main ? " main-stat" : ""}"><div class="label">${label}</div><div class="value">${value}</div></div>`).join("");
        $("warnung").textContent = offline ? "Keine Verbindung – installierte Programme lassen sich trotzdem starten." : "";
        $("liste").innerHTML = progs.map(karte).join("");
        $("version").textContent = daten.version;
        $("wurzel").textContent = daten.wurzel;
    }

    async function laden() {
        setStatus("Suche nach Updates …");
        daten = await window.launcher.liste();
        zeichnen();
        const p = await window.launcher.pruefen();
        neueste = p.programme;
        geprueft = true;
        offline = p.offline;
        const lu = $("launcherUpdate");
        if (p.launcher) {
            lu.innerHTML = `<div>Neue Launcher-Version <b>${esc(p.launcher.version)}</b> verfügbar.</div>` +
                (p.launcher.hatDatei ? `<button class="btn btn-small" id="selbstUpdate" type="button">Herunterladen</button>` : "") +
                `<a href="${esc(p.launcher.seite)}" target="_blank" rel="noopener">Was ist neu?</a>`;
            lu.hidden = false;
        } else {
            lu.hidden = true;
        }
        setStatus("");
        zeichnen();
    }

    async function aktion(was, id) {
        if (was === "ordner") { window.launcher.ordnerOeffnen(id); return; }
        const p = daten.programme.find(x => x.id === id);
        if (was === "starten") {
            const r = await window.launcher.starten(id);
            if (!r.ok) { setStatus(`${p.name} ließ sich nicht starten: ${r.fehler}`); return; }
            const u = r.umzug || { verschoben: [], uebersprungen: [] };
            let text = `${p.name} wird gestartet …`;
            if (u.verschoben.length) text += ` Vorhandene Daten übernommen (${u.verschoben.length} Dateien).`;
            if (u.uebersprungen.length) text += ` Nicht übernommen, weil schon vorhanden: ${u.uebersprungen.join(", ")}.`;
            setStatus(text);
            return;
        }
        if (was === "installieren") {
            beschaeftigt.add(id);
            zeichnen();
            $(`fs-${id}`).hidden = false;
            setStatus(`${p.name} wird heruntergeladen …`);
            const r = await window.launcher.installieren(id);
            beschaeftigt.delete(id);
            if (r.ok) {
                p.installiert = r.installiert;
                setStatus(`${p.name} ${r.installiert.version} ist installiert.`);
            } else {
                setStatus(`Fehler: ${r.fehler}`);
            }
            zeichnen();
        }
    }

    async function selbstUpdate() {
        const btn = $("selbstUpdate");
        btn.disabled = true;
        setStatus("Neue Launcher-Version wird heruntergeladen …");
        const r = await window.launcher.selbstUpdate();
        setStatus(r.ok
            ? `Heruntergeladen: ${r.datei} liegt neben diesem Launcher. Diesen schließen, den neuen starten und den alten löschen.`
            : `Fehler: ${r.fehler}`);
        btn.disabled = false;
    }

    window.launcher.beiFortschritt((id, anteil) => {
        const el = $(`fs-${id}`);
        if (el) { el.hidden = false; el.firstElementChild.style.width = `${Math.round(anteil * 100)}%`; }
        if (id === "launcher") setStatus(`Neue Launcher-Version wird heruntergeladen … ${Math.round(anteil * 100)} %`);
    });

    document.addEventListener("click", e => {
        const btn = e.target.closest("button");
        if (!btn) return;
        if (btn.id === "neuLaden") laden();
        else if (btn.id === "selbstUpdate") selbstUpdate();
        else if (btn.dataset.aktion) aktion(btn.dataset.aktion, btn.dataset.id);
    });

    laden();
})();
