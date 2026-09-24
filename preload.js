// Brücke zwischen Oberfläche und Hauptprozess – nur diese Funktionen.

const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("launcher", {
    /** Programmliste mit installierten Versionen. */
    liste: () => ipcRenderer.invoke("launcher:liste"),
    /** Neueste Versionen aus den Releases (braucht Internet). */
    pruefen: () => ipcRenderer.invoke("launcher:pruefen"),
    installieren: id => ipcRenderer.invoke("launcher:installieren", id),
    starten: id => ipcRenderer.invoke("launcher:starten", id),
    selbstUpdate: () => ipcRenderer.invoke("launcher:selbstUpdate"),
    ordnerOeffnen: id => ipcRenderer.invoke("launcher:ordner", id),
    /** Rückruf (id, anteil 0..1) während eines Downloads. */
    beiFortschritt: rueckruf => ipcRenderer.on("launcher:fortschritt", (_e, id, anteil) => rueckruf(id, anteil)),
});
