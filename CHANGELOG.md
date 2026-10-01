# Änderungsprotokoll

Neueste Version zuerst.

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
