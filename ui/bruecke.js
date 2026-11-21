/* Brücke zwischen Oberfläche und Rust-Teil (Tauri) – stellt dieselben
   Funktionen bereit wie früher die Electron-Brücke (window.launcher), damit
   app.js unverändert bleibt. */

(function () {
    "use strict";
    const tauri = window.__TAURI__;
    if (!tauri || !tauri.core) return;
    const aufruf = tauri.core.invoke;

    window.launcher = {
        /** Programmliste mit installierten Versionen (mitgeliefert, sofort da). */
        liste: () => aufruf("liste"),
        /** Aktuelle Programmliste aus dem Repo (oder null ohne Verbindung). */
        listeOnline: () => aufruf("liste_online"),
        /** Neueste Versionen aus den Releases (braucht Internet). */
        pruefen: () => aufruf("pruefen"),
        installieren: id => aufruf("installieren", { id }),
        starten: id => aufruf("starten", { id }),
        selbstUpdate: () => aufruf("selbst_update"),
        ordnerOeffnen: id => aufruf("ordner_oeffnen", { id }),
        /** Rückruf (id, anteil 0..1) während eines Downloads. */
        beiFortschritt: rueckruf => tauri.event.listen("fortschritt", e => rueckruf(e.payload.id, e.payload.anteil)),
    };

    // Links mit target="_blank" (Release-Seiten) im Standardbrowser öffnen
    document.addEventListener("click", e => {
        const a = e.target.closest && e.target.closest('a[target="_blank"]');
        if (!a || !/^https:\/\//.test(a.href)) return;
        e.preventDefault();
        aufruf("link_oeffnen", { url: a.href }).catch(() => {});
    });
})();
