# Prüfliste OSULZER ImageOptimizer (Abnahme von Hand)

Automatisch getestet sind Bildverarbeitung, Speichern, Konfiguration, Einzelinstanz und Registry
(`cargo test`). Diese Liste deckt ab, was nur im echten Explorer und Fenster prüfbar ist.

Testbilder erzeugen: `cargo run --example beispielbilder -- C:\Temp\bilder 100`
(100 Kamerabilder 6000×4000, ein PNG mit Transparenz, eine kaputte Datei).

## Explorer und Mehrfachauswahl (Spec 7.1)

Eintrag unter „Weitere Optionen anzeigen“ bzw. Shift+Rechtsklick:

- [ ] 1 Bild markiert: Eintrag „ImageOptimizer“ (mit Symbol) erscheint, genau ein Fenster mit dem Bild
- [ ] 5 Bilder: ein Fenster mit 5 Bildern
- [ ] 20 Bilder: Eintrag erscheint (mehr als 15), ein Fenster mit 20 Bildern
- [ ] 100 Bilder: ein Fenster mit 100 Bildern, das Fenster bleibt beim Einlesen bedienbar
- [ ] Gemischte Auswahl (JPG + PNG): Eintrag erscheint, beide Typen in der Liste
- [ ] Fenster offen, weitere Bilder per Rechtsklick: werden angehängt, Fenster kommt nach vorne
- [ ] Dieselben Bilder noch einmal per Rechtsklick: keine doppelten Einträge
- [ ] Ordner mit Umlauten und `&` im Namen (z. B. `Müller & Söhne`) funktioniert
- [ ] Bilder von einem Netzlaufwerk funktionieren

Schlägt der Eintrag bei mehr als 15 Dateien fehl: nicht selbst umbauen, sondern melden
(Ausweichweg laut Design: „Senden an“).

## Abnahme (Spec 8)

- [ ] 30 Kamerabilder mit „Web groß“ ohne weitere Eingabe: landen in `web\`, stehen richtig herum,
      zusammen nur ein Bruchteil der Ausgangsgröße
- [ ] Hochformatfoto aus der Kamera (EXIF-Drehung) steht im Ergebnis aufrecht
- [ ] PNG lässt sich als JPEG speichern; transparente Bereiche werden weiß
- [ ] Ein Bild mit eigenen Werten wird anders behandelt als der Rest; die Vorschau zeigt vorher
      die echte Größe
- [ ] Kein Original wird ohne Rückfrage verändert
- [ ] „Original ersetzen“: Rückfrage erscheint, Originale liegen danach im Papierkorb
- [ ] „Original ersetzen“ auf einem USB-Stick: Original bleibt, Ergebnis als `name-web.jpg`,
      Hinweis „Kein Papierkorb auf diesem Laufwerk“
- [ ] Zielordner `web\` enthält schon Dateien gleichen Namens: Rückfrage Überschreiben /
      Umbenennen / Überspringen
- [ ] Das Fenster erscheint nach dem Rechtsklick ohne spürbare Wartezeit
- [ ] Menüeintrag eintragen und entfernen ohne Administratorrechte (Schaltfläche oben rechts)
- [ ] Ergebnisbilder enthalten keine GPS-Daten (Eigenschaften → Details in Windows)

## KI-Beschreibung

- [ ] Ohne KI-Verbindung: „KI-Beschreibung“ ausgegraut, alles andere wie bisher
- [ ] Einstellungen → KI: „Modelle laden“ füllt die Auswahl, „Verbindung testen“ meldet Erfolg
- [ ] Falsche Adresse: je Zeile eine verständliche Meldung (lokal: „Läuft das Programm …“), Fenster bleibt bedienbar
- [ ] Jeden Anbieter einmal wählen: Adresse (fest bzw. editierbar), Schlüsselfeld, Anleitung, Links und
      Datenschutzzeile passen; Wechsel zwischen Anbietern behält Modell und Schlüssel je Anbieter
- [ ] Anthropic mit echtem Schlüssel: „Modelle laden“ listet Claude-Modelle, „Verbindung testen“ mit
      `claude-haiku-5-5` klappt, KI-Beschreibung über 3 Fotos liefert Texte
- [ ] Erster KI-Lauf mit einem Cloud-Anbieter fragt einmal nach; „Abbrechen“ sendet nichts; nach „Erlauben“
      fragt derselbe Anbieter nicht mehr
- [ ] Falscher Schlüssel: Meldung „Der API-Schlüssel wurde abgelehnt …“, der Schlüssel erscheint nirgends
- [ ] KI-Lauf über 10 Kundenfotos mit Hinweis (z. B. „Hafnermeister Müller, Kachelöfen“): Texte
      passen zum Bild, Fachbegriffe aus dem Hinweis tauchen auf
- [ ] Text in der Seitenleiste ändern, erneuter KI-Lauf: geänderter Text bleibt
- [ ] „Optimieren“: Dateien heißen wie angezeigt; erneut speichern unter anderem Namen legt die alte
      Datei in den Papierkorb
- [ ] WordPress 7: JPEG hochladen → Titel und Alt-Text sind gesetzt; WebP/PNG → Alt-Text gesetzt
- [ ] API-Schlüssel steht nicht in `%APPDATA%\OSULZER\ImageOptimizer\config.json`

## Dateinamen-Regeln

- [ ] „Dateinamen…“ öffnet die Leiste; bei aktiver Regel ist der Knopf farbig
- [ ] Präfix „Mueller“, Suchen „IMG_“: Liste zeigt `mueller-2001.jpg` …; „Optimieren“ speichert genau so
- [ ] Einheitlicher Name „Badsanierung Müller“: `badsanierung-mueller-01.jpg`, `-02`, `-03` in Listenreihenfolge
- [ ] Mit KI-Texten: einheitlicher Name gewinnt, Alt-Text steht trotzdem im XMP
- [ ] Regel ändern nach dem Speichern: Bilder werden wieder „bereit“, erneutes Speichern legt die alte Datei
      in den Papierkorb
- [ ] „Zurücksetzen“: Originalnamen (mit „-web“ bei „Daneben mit Zusatz“)
- [ ] Englische Oberfläche: Texte der Leiste englisch

## Kommandozeile und MCP

- [ ] PowerShell: `osulzer-imageoptimizer.exe optimize <bild> --json` gibt JSON aus, `$LASTEXITCODE` ist 0
- [ ] Kaputte Datei: Exit-Code 1, Meldung auf stderr; falsche Option: Exit-Code 2 mit Hilfe
- [ ] `optimize --name Kunde` auf drei Bilder: `kunde-01..03.jpg` im Ordner `web`, Originale unverändert
- [ ] `describe` mit Cloud-Anbieter ohne Zustimmung: Exit-Code 2 mit Hinweis; nach Zustimmung im Fenster klappt es
- [ ] Einstellungen → Agenten: Befehl für Claude Code kopieren und ausführen; `claude mcp list` zeigt
      `osulzer-imageoptimizer` als verbunden; Claude kann „optimiere C:\…\bild.jpg für das Web“ ausführen
- [ ] Claude Desktop: JSON eintragen, neu starten, Werkzeuge erscheinen

## Umbenennung und Sprachen

- [ ] Rechner mit altem Bildoptimierer (Einstellungen, ggf. API-Schlüssel, Kontextmenü): neue Exe starten →
      Vorgaben und KI-Verbindung sind da, Schlüssel ist übernommen, Kontextmenü zeigt nur noch den neuen Eintrag
- [ ] Zweiter Start ändert nichts mehr; eine geänderte neue `config.json` wird nicht überschrieben
- [ ] Windows auf Englisch, frische Installation: Oberfläche startet englisch
- [ ] Einstellungen → Sprache umschalten: alle Texte wechseln sofort, Kontextmenü-Text im Explorer folgt
- [ ] Sprache der KI-Texte auf Englisch: KI-Beschreibung liefert englische Texte, Dateiname ohne ae/oe/ue
