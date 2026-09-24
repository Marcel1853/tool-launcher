# Tool Launcher

Startet eigene Programme vom USB-Stick (oder aus einem beliebigen Ordner) und
hält sie aktuell: installieren, aktualisieren, starten – ohne Anmeldung,
ohne Installation.

Zurzeit dabei: **Paletten-Packschema**.

## Herunterladen

Unter [Releases](https://github.com/Marcel1853/tool-launcher/releases) die
neueste Version laden:

- **Windows**: `Tool-Launcher-<Version>.exe`
- **Linux**: `Tool-Launcher-<Version>.AppImage` (einmalig als ausführbar
  markieren)

Die Datei in den Ordner legen, in dem alles liegen soll (etwa ganz vorne auf
den Stick), und starten.

## Aufbau

```
Stick/
  Tool-Launcher-1.0.0.exe
  Programme/
    Paletten-Packschema/      das Programm + version.json (wird bei Updates ersetzt)
  Daten/
    Paletten-Packschema/      Schemas, Einstellungen … (bleibt immer)
```

- **Programme/** verwaltet der Launcher selbst. Eine neue Version wird erst
  vollständig heruntergeladen und geprüft, danach wird die alte gelöscht.
- **Daten/** fasst der Launcher bei Updates nie an. Jedes Programm bekommt
  beim Start seinen eigenen Unterordner vorgegeben.
- **Umzug:** Lagen die Daten eines Programms bisher direkt neben ihm (z. B.
  `Paletten-Packschema/Daten/`), holt der Launcher sie beim ersten Start in
  `Daten/<Programm>/`. Kopiert, verglichen, erst dann wird das Original
  entfernt; was am Ziel schon liegt, wird nie überschrieben. Im alten Ordner
  bleibt eine `UMGEZOGEN.txt`.
- **Ohne Internet** lassen sich installierte Programme ganz normal starten.

## Updates des Launchers

Gibt es eine neuere Launcher-Version, zeigt er das oben an und lädt sie auf
Wunsch neben sich herunter. Eine laufende Exe kann sich nicht selbst
ersetzen – danach den alten Launcher schließen, den neuen starten und den
alten löschen.

## Programme hinzufügen

Die Liste steht in [`apps.json`](apps.json) und wird direkt aus `main`
gelesen – ein neues Programm braucht also keinen neuen Launcher. Je Eintrag:

| Feld | Bedeutung |
|---|---|
| `id` | eindeutiger Schlüssel |
| `name`, `beschreibung` | Anzeige |
| `icon` | Symbol (PNG), Pfad im Launcher-Repo, z. B. `icons/paletten-packschema.png` |
| `repo` | öffentliches GitHub-Repo mit den Releases – für alle eigenen Programme das Sammel-Repo `Marcel1853/tool-releases` |
| `tag` | Kennung am Anfang des Release-Tags, z. B. `paletten-packschema-v` (Tag `paletten-packschema-v1.6.0`) – trennt die Programme im Sammel-Repo |
| `ordner` | Unterordner in `Programme/` |
| `daten` | Unterordner in `Daten/` – wird dem Programm als `LAUNCHER_DATEN_DIR` übergeben |
| `dateien` | Muster (regulärer Ausdruck) für die Release-Datei je System (`win32`, `linux`) |
| `umzug` | optional: alte Daten-Orte (`von`, relativ zum Launcher) und Dateinamen |

Das Programm selbst muss `LAUNCHER_DATEN_DIR` als Daten-Ordner verwenden,
wenn die Variable gesetzt ist.

## Entwickeln

```sh
npm install
npm start                                  # aus den Quellen, Wurzel = Projektordner
LAUNCHER_ROOT=/pfad/zum/testordner npm start   # anderer Wurzelordner
npm run build                              # beide Fassungen nach dist/
```

Veröffentlichen: Version in `package.json` hochzählen, Abschnitt im
`CHANGELOG.md` schreiben, auf `main` pushen – der Workflow baut und legt das
Release an.
