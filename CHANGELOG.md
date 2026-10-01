# Änderungsprotokoll

Neueste Version zuerst.

## [1.2.0] – 2026-10-01

### Behoben

- Launcher hing im Firmennetz: Die Abfragen liefen über Nodes eigenes
  fetch, das die Proxy-Einstellungen des Systems nicht kennt – jede Abfrage
  wartete bis zur Zeitgrenze, nacheinander, bevor überhaupt etwas angezeigt
  wurde. Jetzt über Chromium (wie ein Browser, mit Proxy), alle Programme
  gleichzeitig, höchstens 8 Sekunden.
- Das Fenster zeigt die Programme sofort aus der mitgelieferten Liste; die
  aktuelle Liste und die Versionen kommen danach.
- Downloads brechen ab, wenn 30 Sekunden lang nichts ankommt, statt ewig
  zu hängen.
- Unter Linux blieb nach einem Selbst-Update (und solange ein gestartetes
  Programm lief) der alte Launcher im Hintergrund hängen, weil das neue
  Programm noch Dateien aus dessen AppImage geerbt hatte. Programme werden
  jetzt ohne diese geerbten Dateien gestartet.

## [1.1.0] – 2026-10-01

### Hinzugefügt

- Selbst-Update: Gibt es eine neuere Launcher-Version, lädt der Launcher sie
  beim Start neben sich herunter, startet sie und beendet sich; die neue
  Version löscht danach die alte Datei und meldet „Launcher aktualisiert“.
  Aus den Quellen gestartet wird nur heruntergeladen.

### Behoben

- „Keine Verbindung“, obwohl Internet da war: Die GitHub-API erlaubt ohne
  Anmeldung nur 60 Abfragen pro Stunde je Internetanschluss – in der Firma
  für alle Rechner zusammen. Versionen kommen jetzt aus dem Release-Feed auf
  github.com, Downloads über die feste Adresse; die API nur noch als
  Rückfall. Dafür in `apps.json` das Feld `download` (Dateiname mit
  `{version}`).

## [1.0.0] – 2026-09-24

### Hinzugefügt

- Erste Fassung: Programmliste aus `apps.json` (online aus dem Repo, ohne
  Internet die mitgelieferte), Installieren, Aktualisieren und Starten mit
  Fortschrittsanzeige und Änderungstext der neuen Version.
- Programme liegen in `Programme/<Name>/`, ihre Daten in `Daten/<Name>/`;
  Updates fassen die Daten nie an.
- Einmaliger Umzug vorhandener Daten (z. B. aus
  `Paletten-Packschema/Daten/`) beim ersten Start über den Launcher.
- Hinweis auf neue Launcher-Versionen, Download direkt neben den Launcher.
- Erstes Programm: Paletten-Packschema.
