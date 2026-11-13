//! Interface translations. Add a locale catalog and a Language variant to extend.
//! English source text is the key and the fallback for untranslated messages.
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "de")]
    German,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::German];
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

/// Set before constructing views, or refresh views after changing the language.
pub fn set_language(language: Language) {
    LANGUAGE.store(language as u8, Ordering::Relaxed);
}

pub fn translate(language: Language, text: &str) -> &str {
    if language == Language::German {
        match text {
            "Expand" => "Ausklappen",
            "Collapse" => "Einklappen",
            "Select a row to see its engine line" => {
                "Zeile auswählen, um die Engine-Variante zu sehen"
            }
            "{} new games, {} already stored" => "{} neue Partien, {} bereits gespeichert",
            "Connecting to {}…" => "Verbindung zu {} wird hergestellt…",
            "Fetching archive list from {}…" => "Archivliste von {} wird abgerufen…",
            "Receiving games from {}…" => "Partien von {} werden empfangen…",
            "Saving downloaded games…" => "Heruntergeladene Partien werden gespeichert…",
            "Cancelling download…" => "Download wird abgebrochen…",

            "Enter a valid starting position." => "Eine gültige Startstellung eingeben.",
            "Reference FEN" => "Referenz-FEN",
            "Position patterns" => "Stellungsmuster",
            "Same pawn structure" => "Gleiche Bauernstruktur",
            "Same material" => "Gleiches Material",
            "Build structure index" => "Strukturindex erstellen",
            "Build the structure index to search" => "Zum Suchen den Strukturindex erstellen",
            "Enter a valid reference FEN" => "Eine gültige Referenz-FEN eingeben",
            "Exact matches, either side to move" => "Exakte Treffer, beide Zugfarben",
            "Structure index ready" => "Strukturindex bereit",
            "Structure indexing cancelled" => "Strukturindizierung abgebrochen",

            "Chess960" => "Chess960",
            "New Chess960 game…" => "Neue Chess960-Partie…",
            "New Chess960 game" => "Neue Chess960-Partie",
            "No Chess960 support" => "Keine Chess960-Unterstützung",
            "Enter a number from 0 to 959" => "Eine Zahl von 0 bis 959 eingeben",
            "Searching…" => "Suche…",
            "Some databases could not be searched" => {
                "Einige Datenbanken konnten nicht durchsucht werden"
            }
            "No matching games" => "Keine passenden Partien",
            "Matching games" => "Passende Partien",
            "Could not open game" => "Partie konnte nicht geöffnet werden",
            "Analyse" => "Analyse",
            "Database" => "Datenbank",
            "Train" => "Training",
            "Play" => "Spielen",
            "Settings" => "Einstellungen",
            "Tournament" => "Turnier",
            "General" => "Allgemein",
            "Fast updates" => "Fast-Updates",
            "Installed version" => "Installierte Version",
            "Available version" => "Verfügbare Version",
            "Check for updates" => "Nach Updates suchen",
            "Download and install update" => "Update herunterladen und installieren",
            "Check for a new Fast release. Updates keep your settings, databases and engines." => {
                "Nach einer neuen Fast-Version suchen. Einstellungen, Datenbanken und Engines bleiben erhalten."
            }
            "Checking for updates…" => "Es wird nach Updates gesucht…",
            "A new version is available." => "Eine neue Version ist verfügbar.",
            "Fast is up to date." => "Fast ist auf dem neuesten Stand.",
            "Could not check for updates. Try again later." => {
                "Updates konnten nicht geprüft werden. Bitte später erneut versuchen."
            }
            "Downloading and verifying update…" => "Update wird heruntergeladen und geprüft…",
            "Update installed. Close and reopen Fast to use the new version." => {
                "Update installiert. Fast schließen und erneut öffnen, um die neue Version zu verwenden."
            }
            "Installer opened. Finish installation, then close and reopen Fast." => {
                "Installationsprogramm geöffnet. Installation abschließen, dann Fast schließen und erneut öffnen."
            }
            "Update failed. You can retry." => "Update fehlgeschlagen. Bitte erneut versuchen.",
            "Board" => "Brett",
            "Board & pieces" => "Brett und Figuren",
            "Engines" => "Engines",
            "Databases" => "Datenbanken",
            "Keyboard shortcuts" => "Tastenkürzel",
            "Shortcuts" => "Tastenkürzel",
            "Accounts" => "Konten",
            "Folders" => "Ordner",
            "OTHER FILES" => "ANDERE DATEIEN",
            "FREE" => "FREI",
            "Drive" => "Laufwerk",
            "Everything else" => "Alles andere",
            "Disk use" => "Speicherbedarf",
            "Refresh" => "Aktualisieren",
            "Measuring…" => "Wird gemessen…",
            "Contents" => "Inhalt",
            "Size" => "Größe",
            "Total" => "Gesamt",
            "Analysis cache" => "Analyse-Cache",
            "Notes" => "Notizen",
            "Recovery" => "Wiederherstellung",
            "Opening books" => "Eröffnungsbücher",
            "Tournaments" => "Turniere",
            "Tools" => "Werkzeuge",
            "Preferences, engines, shortcuts and panel sizes" => {
                "Einstellungen, Engines, Tastenkürzel und Panelgrößen"
            }
            "Imported games and My games" => "Importierte Partien und Meine Partien",
            "Engine lines saved for positions you analysed" => {
                "Gespeicherte Engine-Varianten analysierter Stellungen"
            }
            "Training lines and review schedule" => "Trainingsvarianten und Wiederholungsplan",
            "Comments on positions" => "Kommentare zu Stellungen",
            "Open games, restored after a crash" => {
                "Offene Partien, nach einem Absturz wiederhergestellt"
            }
            "Engines downloaded from the catalog" => "Aus dem Katalog geladene Engines",
            "Downloaded books for Play and Tournament" => "Geladene Bücher für Spielen und Turnier",
            "Saved tournament runs and their games" => "Gespeicherte Turniere und ihre Partien",
            "Fastchess, used to run tournaments" => "Fastchess, zum Ausführen von Turnieren",
            "Files Fast does not list above" => "Dateien, die oben nicht aufgeführt sind",
            "Everything stays on this computer. Back up these folders to keep your games, repertoire and analysis." => {
                "Alles bleibt auf diesem Computer. Sichern Sie diese Ordner, um Partien, Repertoire und Analysen zu behalten."
            }
            "Licenses" => "Lizenzen",
            "Thanks" => "Danksagung",
            "Changelog" => "Änderungen",
            "Variant" => "Variante",
            "Chess960 needs a book of Chess960 start positions." => {
                "Chess960 braucht ein Buch mit Chess960-Startstellungen."
            }
            "Elo model" => "Elo-Modell",
            "Engine arguments" => "Engine-Argumente",
            "Never swap colours" => "Farben nie tauschen",
            "Reverse colours" => "Farben umkehren",
            "Tablebase adjudication" => "Endspieldatenbank-Abbruch",
            "Syzygy folders" => "Syzygy-Ordner",
            "Up to pieces" => "Bis Steine",
            "Wins and draws" => "Siege und Remis",
            "Wins only" => "Nur Siege",
            "Draws only" => "Nur Remis",
            "Ignore the 50-move rule" => "50-Züge-Regel ignorieren",
            "Saved games" => "Gespeicherte Partien",
            "Games in LAN or UCI may not open in Analyse." => {
                "Partien in LAN oder UCI lassen sich eventuell nicht in Analyse öffnen."
            }
            "Record with each move" => "Bei jedem Zug speichern",
            "Selective depth" => "Selektive Tiefe",
            "Hash usage" => "Hash-Auslastung",
            "Tablebase hits" => "Endspieldatenbank-Treffer",
            "Full line" => "Ganze Variante",
            "Time left" => "Restzeit",
            "Latency" => "Latenz",
            "Minimal PGN" => "Minimales PGN",
            "Copy engine output matching" => "Engine-Ausgabe übernehmen, die passt auf",
            "Visit En Croissant" => "En Croissant besuchen",
            "Thanks to En Croissant and its contributors for a carefully handcrafted chess GUI with a rich set of features. Its project history and community requests were valuable resources for the implementation of this GUI." => {
                "Danke an En Croissant und alle Mitwirkenden für eine sorgfältig von Hand gestaltete Schachoberfläche mit vielen Funktionen. Die Projektgeschichte und die Wünsche der Community waren wertvolle Ressourcen für die Umsetzung dieser Oberfläche."
            }
            "Licences" => "Lizenzen",
            "Appearance" => "Darstellung",
            "Theme" => "Design",
            "Language" => "Sprache",
            "Applies to the entire interface" => "Gilt für die gesamte Oberfläche",
            "You" => "Spieler",
            "Files" => "Dateien",
            "Player name" => "Spielername",
            "Player aliases" => "Weitere Spielernamen",
            "Separate names with semicolons" => "Namen durch Semikolons trennen",
            "Other names for My games and leak detection. Separate names with semicolons." => {
                "Weitere Namen für Meine Partien und Schwachstellen. Namen durch Semikolons trennen."
            }
            "Move to the removed folder" => "In den Ordner removed verschieben",
            "Removed databases are kept in the removed folder." => {
                "Entfernte Datenbanken bleiben im Ordner removed erhalten."
            }
            "Your name in game headers" => "Ihr Name in den Partiedaten",
            "Name" => "Name",
            "Rating" => "Wertung",
            "White" => "Weiß",
            "Black" => "Schwarz",
            "Cancel" => "Abbrechen",
            "Close" => "Schließen",
            "Open" => "Öffnen",
            "Save" => "Speichern",
            "Delete" => "Löschen",
            "Remove" => "Entfernen",
            "Could not remove database" => "Datenbank konnte nicht entfernt werden",
            "Add" => "Hinzufügen",
            "Edit" => "Bearbeiten",
            "Reset" => "Zurücksetzen",
            "Apply" => "Anwenden",
            "Done" => "Fertig",
            "Clear" => "Leeren",
            "Search" => "Suchen",
            "Import" => "Importieren",
            "Export" => "Exportieren",
            "Reopen" => "Wieder öffnen",
            "Repertoires" => "Repertoires",
            "New game" => "Neue Partie",
            "New tab" => "Neuer Tab",
            "Close all tabs" => "Alle Tabs schließen",
            "Close tab" => "Tab schließen",
            "Reopen closed tab" => "Geschlossenen Tab wieder öffnen",
            "Open PGN or FEN…" => "PGN oder FEN öffnen…",
            "Save to My games" => "In Meine Partien speichern",
            "Save as PGN…" => "Als PGN speichern…",
            "Quit" => "Beenden",
            "File" => "Datei",
            "Copy FEN" => "FEN kopieren",
            "Copy PGN" => "PGN kopieren",
            "Paste game" => "Partie einfügen",
            "View" => "Ansicht",
            "Game" => "Partie",
            "Engine" => "Engine",
            "Window" => "Fenster",
            "Help" => "Hilfe",
            "About" => "Über",
            "Minimize" => "Minimieren",
            "Maximize" => "Maximieren",
            "Restore" => "Wiederherstellen",
            "Flip board" => "Brett drehen",
            "First move" => "Erster Zug",
            "Last move" => "Letzter Zug",
            "Next move" => "Nächster Zug",
            "Previous move" => "Vorheriger Zug",
            "Moves" => "Züge",
            "Move" => "Zug",
            "Depth" => "Tiefe",
            "Time" => "Zeit",
            "Eval" => "Bewertung",
            "Bars" => "Balken",
            "Analysis" => "Analyse",
            "Backwards analysis" => "Rückwärtsanalyse",
            "Start" => "Starten",
            "Stop" => "Stoppen",
            "Pause" => "Pausieren",
            "Leaving an Analyse tab" => "Beim Verlassen eines Analyse-Tabs",
            "Keep analysing" => "Weiter analysieren",
            "Analysis paused" => "Analyse pausiert",
            "Kibitz engine" => "Kiebitz-Engine",
            "Kibitz" => "Kiebitz",
            "Line previews" => "Variantenvorschau",
            "Analyse with a kibitz engine" => "Mit einer Kiebitz-Engine analysieren",
            "First analysis engine" => "Erste Analyse-Engine",
            "Clicking a move in Line preview analyses that position with a second engine, shown below the line, while the main analysis keeps searching the game position. Off: the main engines follow the preview." => {
                "Ein Klick auf einen Zug in der Variantenvorschau analysiert diese Stellung mit einer zweiten Engine, angezeigt unter der Variante, während die Hauptanalyse die Partiestellung weiter durchsucht. Aus: Die Haupt-Engines folgen der Vorschau."
            }
            "Engines are searching in this tab" => "In diesem Tab rechnen Engines",
            "The engines keep searching while a tab other than Analyse is shown. Switching to another Analyse tab hands them over." => {
                "Die Engines rechnen weiter, solange ein anderer Tab als Analyse offen ist. Ein anderer Analyse-Tab übernimmt sie."
            }
            "The search pauses and keeps its lines. The tab shows a pause mark and stays paused when you come back, until you resume (P)." => {
                "Die Suche pausiert und behält ihre Varianten. Der Tab zeigt ein Pausenzeichen und bleibt pausiert, bis du sie fortsetzt (P)."
            }
            "The search stops and starts again from your position when you come back." => {
                "Die Suche stoppt und beginnt neu bei deiner Stellung, wenn du zurückkommst."
            }
            "Resume" => "Fortsetzen",
            "Continue" => "Fortfahren",
            "Result" => "Ergebnis",
            "Event" => "Veranstaltung",
            "Site" => "Ort",
            "Date" => "Datum",
            "Round" => "Runde",
            "White player" => "Weißspieler",
            "Black player" => "Schwarzspieler",
            "My games" => "Meine Partien",
            "All games" => "Alle Partien",
            "New database" => "Neue Datenbank",
            "Open database" => "Datenbank öffnen",
            "No games" => "Keine Partien",
            "Training" => "Training",
            "Repertoire" => "Repertoire",
            "New repertoire" => "Neues Repertoire",
            "New tournament" => "Neues Turnier",
            "Options" => "Optionen",
            "Details" => "Details",
            "Profile" => "Profil",
            "Profiles" => "Profile",
            "Default" => "Standard",
            "Enabled" => "Aktiviert",
            "Disabled" => "Deaktiviert",
            "On" => "Ein",
            "Off" => "Aus",
            "None" => "Keine",
            "Filters" => "Filter",
            "Choose database" => "Datenbank auswählen",
            "Search databases" => "Datenbanken suchen",
            "PGN files" => "PGN-Dateien",
            "All files" => "Alle Dateien",
            "Position {} / {}" => "Stellung {} / {}",
            "{} matching games" => "{} passende Partien",
            "Filters ({})" => "Filter ({})",
            "Export matching games" => "Passende Partien exportieren",
            "Create empty database" => "Leere Datenbank erstellen",
            "Review {} / {}" => "Analyse {} / {}",
            "Stop review" => "Analyse stoppen",
            "HTTPS download URL" => "HTTPS-Download-URL",
            "Cancel indexing" => "Indexierung abbrechen",
            "Manage" => "Verwalten",
            "Local" => "Lokal",
            "Web" => "Web",
            "Players" => "Spieler",
            "Events" => "Turniere",
            "Add database" => "Datenbank hinzufügen",
            "Manage database" => "Datenbank verwalten",
            "Database name" => "Datenbankname",
            "PGN, PGN.gz, ZIP or Fast SQLite file" => "PGN-, PGN.gz-, ZIP- oder Fast-SQLite-Datei",
            "Player to replace" => "Zu ersetzender Spieler",
            "Player to keep" => "Zu behaltender Spieler",
            "Enter a name and a source." => "Einen Namen und eine Quelle eingeben.",
            "Preparing database…" => "Datenbank wird vorbereitet…",
            "Database added." => "Datenbank hinzugefügt.",
            "Updated {} games." => "{} Partien aktualisiert.",
            "No matching games to remove." => "Keine passenden Partien zum Entfernen.",
            "PGN download URL" => "PGN-Download-URL",
            "Local file" => "Lokale Datei",
            "Download a PGN, PGN.gz or ZIP file over HTTPS." => {
                "Eine PGN-, PGN.gz- oder ZIP-Datei über HTTPS herunterladen."
            }
            "Import PGN, PGN.gz or ZIP, or copy a Fast SQLite database." => {
                "PGN, PGN.gz oder ZIP importieren oder eine Fast-SQLite-Datenbank kopieren."
            }
            "The Week in Chess" => "The Week in Chess",
            "Download the latest weekly issue. Free for personal use." => {
                "Die neueste Wochenausgabe herunterladen. Für den privaten Gebrauch kostenlos."
            }
            "Install latest issue" => "Neueste Ausgabe installieren",
            "Use as reference database" => "Als Referenzdatenbank verwenden",
            "Position index ready" => "Stellungsindex bereit",
            "Rebuild structure index" => "Strukturindex neu aufbauen",
            "Explore games" => "Partien durchsuchen",
            "Merge players" => "Spieler zusammenführen",
            "Replace this exact player name on both sides of every game." => {
                "Diesen genauen Spielernamen für beide Farben in allen Partien ersetzen."
            }
            "Clean up" => "Bereinigen",
            "Review the number of games before confirming removal. Different annotations are kept." => {
                "Die Anzahl vor dem Entfernen prüfen. Unterschiedliche Kommentare bleiben erhalten."
            }
            "Remove duplicates" => "Duplikate entfernen",
            "Remove empty games" => "Leere Partien entfernen",
            "Indexing {} / {} games" => "{} / {} Partien werden indexiert",
            "Remove database" => "Datenbank entfernen",
            "Add games" => "Partien hinzufügen",
            "Exported {} games." => "{} Partien exportiert.",
            "Downloaded {} MB" => "{} MB heruntergeladen",
            "Imported {} games" => "{} Partien importiert",
            "Played" => "Gespielt",
            "White / Draw / Black" => "Weiß / Remis / Schwarz",
            "White wins: {}%, draws: {}%, Black wins: {}%, unfinished: {}%" => {
                "Weiß gewinnt: {}%, Remis: {}%, Schwarz gewinnt: {}%, unbeendet: {}%"
            }
            "Add a database in Settings › Databases." => {
                "Eine Datenbank unter Einstellungen › Datenbanken hinzufügen."
            }
            "No games reach this position." => "Keine Partie erreicht diese Stellung.",
            "Could not query database: {}" => "Datenbankabfrage fehlgeschlagen: {}",
            "Unlimited time" => "Unbegrenzte Zeit",
            "Time control (minutes+increment)" => "Bedenkzeit (Minuten+Inkrement)",
            "Per-move search limit" => "Suchgrenze pro Zug",
            "Enter a custom time such as 12+3, or a positive search limit." => {
                "Eine Bedenkzeit wie 12+3 oder eine positive Suchgrenze eingeben."
            }
            "Opening book (.pgn or .epd)" => "Eröffnungsbuch (.pgn oder .epd)",
            "Optional local file. Uses the first game or position." => {
                "Optionale lokale Datei. Verwendet die erste Partie oder Stellung."
            }
            "Enter a positive search limit." => "Eine positive Suchgrenze eingeben.",
            "Enter a smaller depth." => "Eine kleinere Tiefe eingeben.",
            "Unlimited" => "Unbegrenzt",
            "Infinite" => "Unbegrenzt",
            "Seconds" => "Sekunden",
            "Milliseconds" => "Millisekunden",
            "Minutes" => "Minuten",
            "Hours" => "Stunden",
            "Increment" => "Inkrement",
            "Time per move" => "Zeit pro Zug",
            "Enter a positive time and a nonnegative increment, or a positive search limit." => {
                "Eine positive Zeit und ein nicht negatives Inkrement oder eine positive Suchgrenze eingeben."
            }
            "Nodes" => "Knoten",
            "Threads" => "Threads",
            "Memory" => "Speicher",
            "Storage" => "Speicher",
            "Data" => "Daten",
            "Back up" => "Sichern",
            "Backup" => "Sicherung",
            "Restore backup" => "Sicherung wiederherstellen",
            "Coordinates" => "Koordinaten",
            "Notation" => "Notation",
            "Animation" => "Animation",
            "Move sounds" => "Zuggeräusche",
            "Volume" => "Lautstärke",
            "Sound" => "Ton",
            "Pieces" => "Figuren",
            "Piece set" => "Figurensatz",
            "Board theme" => "Brettdesign",
            "Show legal moves" => "Legale Züge anzeigen",
            "Highlight last move" => "Letzten Zug hervorheben",
            "Badge moves from live analysis" => "Züge aus der Live-Analyse bewerten",
            "Auto queen" => "Automatisch zur Dame umwandeln",
            "Automatic queen promotion" => "Automatische Umwandlung zur Dame",
            "On squares" => "Auf den Feldern",
            "Outside" => "Außerhalb",
            "Table" => "Tabelle",
            "Inline" => "Fließtext",
            "All moves" => "Alle Züge",
            "My moves" => "Meine Züge",
            "Show what you played and your repertoire for each move" => {
                "Zeigt zu jedem Zug, was du gespielt hast, und dein Repertoire"
            }
            "Best move" => "Bester Zug",
            "Quality arrows" => "Qualitätspfeile",
            "Sort by depth" => "Nach Tiefe sortieren",
            "Sort by evaluation" => "Nach Bewertung sortieren",
            "Engine output" => "Engine-Ausgabe",
            "Toggle engine" => "Engine umschalten",
            "Pause engine" => "Engine pausieren",
            "More lines" => "Mehr Varianten",
            "Fewer lines" => "Weniger Varianten",
            "Next tab" => "Nächster Tab",
            "Previous tab" => "Vorheriger Tab",
            "Next tab, second key" => "Nächster Tab, zweite Taste",
            "Previous tab, second key" => "Vorheriger Tab, zweite Taste",
            "Go back" => "Zurück",
            "Go forward" => "Vorwärts",
            "Navigation" => "Navigation",
            "Reset all" => "Alles zurücksetzen",
            "Reset to defaults" => "Auf Standard zurücksetzen",
            "Bundled assets" => "Mitgelieferte Ressourcen",
            "Libraries" => "Bibliotheken",
            "Offline by design" => "Für Offline-Nutzung entwickelt",
            "Connection" => "Verbindung",
            "Download" => "Herunterladen",
            "Install" => "Installieren",
            "Installed" => "Installiert",
            "Available" => "Verfügbar",
            "Browse…" => "Durchsuchen…",
            "Choose…" => "Auswählen…",
            "Select" => "Auswählen",
            "Description" => "Beschreibung",
            "Status" => "Status",
            "Score" => "Ergebnis",
            "Opponent" => "Gegner",
            "Human" => "Mensch",
            "Computer" => "Computer",
            "Resign" => "Aufgeben",
            "Offer draw" => "Remis anbieten",
            "Accept" => "Annehmen",
            "Decline" => "Ablehnen",
            "Undo" => "Rückgängig",
            "Retry" => "Erneut versuchen",
            "Copy" => "Kopieren",
            "Paste" => "Einfügen",
            "Comment" => "Kommentar",
            "Comments" => "Kommentare",
            "Variations" => "Varianten",
            "Main line" => "Hauptvariante",
            "Promotion" => "Umwandlung",
            "Queen" => "Dame",
            "Rook" => "Turm",
            "Bishop" => "Läufer",
            "Knight" => "Springer",
            "All" => "Alle",
            "Any" => "Beliebig",
            "Games" => "Partien",
            "Rounds" => "Runden",
            "Concurrency" => "Gleichzeitige Partien",
            "Automatic" => "Automatisch",
            "No limit" => "Keine Begrenzung",
            "No book" => "Kein Eröffnungsbuch",
            "Custom file…" => "Eigene Datei…",
            "Preview" => "Vorschau",
            "Move sound" => "Zuggeräusch",
            "Sound volume" => "Lautstärke",
            "Move animation" => "Zuganimation",
            "Board behaviour" => "Brettverhalten",
            "For move sounds" => "Für Zuggeräusche",
            "Choose a sound and preview it at your current volume" => {
                "Wählen Sie einen Klang und hören Sie ihn mit der aktuellen Lautstärke an"
            }
            "Retry engine check" => "Engine erneut prüfen",
            "Engine check failed. Retry using the installed files" => {
                "Engine-Prüfung fehlgeschlagen. Mit den installierten Dateien erneut versuchen"
            }
            "Download & install" => "Herunterladen und installieren",
            "Add from file" => "Aus Datei hinzufügen",
            "Source" => "Quelle",
            "None yet" => "Noch keine",
            "Use" => "Verwenden",
            "Download now" => "Jetzt herunterladen",
            "Cancel download" => "Download abbrechen",
            "Download cancelled." => "Download abgebrochen.",
            "Command copied" => "Befehl kopiert",
            "Engine executable" => "Engine-Programm",
            "Decisive" => "Entschieden",
            "Adjudicated" => "Abgeschätzt",
            "Repetition" => "Wiederholung",
            "Show all" => "Alle anzeigen",
            "CPU cores to use" => "Anzahl der CPU-Kerne",
            "Press a key… (Esc cancels)" => "Taste drücken… (Esc bricht ab)",
            "No engines yet. Add a UCI engine executable with Add from file." => {
                "Noch keine Engines. Fügen Sie mit „Aus Datei hinzufügen“ eine UCI-Engine hinzu."
            }
            "No databases yet. Import a PGN from the Database screen." => {
                "Noch keine Datenbanken. Importieren Sie eine PGN-Datei im Bereich Datenbank."
            }
            "Use this engine for" => "Diese Engine verwenden für",
            "A board, engine analysis and the move list for a new position or game." => {
                "Brett, Engine-Analyse und Zugliste für eine neue Stellung oder Partie."
            }
            "Play a game against an engine, with clocks." => {
                "Spielen Sie mit Bedenkzeit gegen eine Engine."
            }
            "Browse, search and import your game collections." => {
                "Partiesammlungen durchsuchen und importieren."
            }
            "Engine-vs-engine matches run with fastchess." => "Engine-Turniere mit fastchess.",
            "Drill your repertoire with spaced repetition." => {
                "Repertoire mit verteilter Wiederholung üben."
            }
            "Analyse backwards" => "Rückwärts analysieren",
            "Acc." => "Genauigkeit",
            "Add an engine in Settings › Engines to play." => {
                "Fügen Sie unter Einstellungen › Engines eine Engine zum Spielen hinzu."
            }
            "Add as variation" => "Als Variante hinzufügen",
            "Add executable…" => "Programm hinzufügen…",
            "Add line" => "Variante hinzufügen",
            "Add to repertoire" => "Zum Repertoire hinzufügen",
            "Adjudication" => "Abschätzung",
            "Advanced settings" => "Erweiterte Einstellungen",
            "All options" => "Alle Optionen",
            "Analyse this game" => "Diese Partie analysieren",
            "Analyse →" => "Analysieren →",
            "Asset" => "Ressource",
            "Back to game" => "Zurück zur Partie",
            "Batch review" => "Stapelprüfung",
            "By" => "Von",
            "Cancel restore" => "Wiederherstellung abbrechen",
            "Castling" => "Rochade",
            "Check for update" => "Nach Updates suchen",
            "Cloud" => "Cloud",
            "Copy all" => "Alles kopieren",
            "Copy command" => "Befehl kopieren",
            "Copy config" => "Konfiguration kopieren",
            "Custom" => "Benutzerdefiniert",
            "Download an engine" => "Engine herunterladen",
            "Download engine" => "Engine herunterladen",
            "Correct: {}. Press Repeat to practise this position again." => {
                "Richtig: {}. Drücke Wiederholen, um diese Stellung erneut zu üben."
            }
            "Correct: {}. Hint used: choose Repeat or Hard." => {
                "Richtig: {}. Hinweis verwendet: Wähle Wiederholen oder Schwer."
            }
            "Correct: {}. How well did you know it?" => "Richtig: {}. Wie gut wusstest du es?",
            "Answer revealed. This position will repeat." => {
                "Antwort aufgedeckt. Diese Stellung wird wiederholt."
            }
            "A mistake or reveal means this position will repeat." => {
                "Nach einem Fehler oder Aufdecken wird diese Stellung wiederholt."
            }
            "Hint used. The highest available grade is Hard." => {
                "Hinweis verwendet. Die höchste verfügbare Bewertung ist Schwer."
            }
            "Completed" => "Abgeschlossen",
            "remaining" => "verbleibend",
            "Session complete" => "Einheit abgeschlossen",
            "Your repertoire is empty" => "Dein Repertoire ist leer",
            "Nothing scheduled today" => "Heute ist nichts geplant",
            "All session positions completed. Come back when more are due, or add a line." => {
                "Alle Stellungen abgeschlossen. Komm wieder, wenn weitere fällig sind, oder füge eine Variante hinzu."
            }
            "Use Add line or Import PGN to prepare your first positions." => {
                "Bereite deine ersten Stellungen mit Variante hinzufügen oder PGN importieren vor."
            }
            "Come back when positions are due, or use Add line to learn something new." => {
                "Komm wieder, wenn Stellungen fällig sind, oder lerne mit Variante hinzufügen etwas Neues."
            }
            "Repeat" => "Wiederholen",
            "Drill these lines" => "Diese Varianten üben",
            "Duplicate" => "Duplizieren",
            "Earlier tournaments" => "Frühere Turniere",
            "Engine against engine" => "Engine gegen Engine",
            "Export PGN" => "PGN exportieren",
            "Export complete backup" => "Vollständige Sicherung exportieren",
            "Filter" => "Filter",
            "Find gaps" => "Lücken finden",
            "Import PGN" => "PGN importieren",
            "Follow" => "Folgen",
            "Game graph" => "Partiediagramm",
            "Hash and threads, pairing, adjudication, recovery, extra options" => {
                "Hash und Threads, Paarung, Abschätzung, Wiederherstellung, weitere Optionen"
            }
            "Held while selected. Ctrl+C copies, Esc releases." => {
                "Während der Auswahl angehalten. Strg+C kopiert, Esc gibt frei."
            }
            "Info lines" => "Infozeilen",
            "Leaks from your games" => "Abweichungen in Ihren Partien",
            "Left rep." => "Repertoire verlassen",
            "Licence" => "Lizenz",
            "Line" => "Variante",
            "Line preview" => "Variantenvorschau",
            "Lines" => "Varianten",
            "Load config file…" => "Konfigurationsdatei laden…",
            "Log" => "Protokoll",
            "Nothing logged yet." => "Noch nichts protokolliert.",
            "Moves from here" => "Züge ab hier",
            "New game vs engine" => "Neue Partie gegen Engine",
            "Vs engine" => "Gegen Engine",
            "No search data yet. Scores and search effort appear after each move." => {
                "Noch keine Suchdaten. Bewertungen und Suchaufwand erscheinen nach jedem Zug."
            }
            "Engine vs engine" => "Engine gegen Engine",
            "Game stopped" => "Partie abgebrochen",
            "your move" => "am Zug",
            "No databases yet. Create one with New or import a PGN." => {
                "Noch keine Datenbanken. Erstellen Sie eine mit Neu oder importieren Sie eine PGN-Datei."
            }
            "No engines yet. Add one in Settings › Engines." => {
                "Noch keine Engines. Fügen Sie unter Einstellungen › Engines eine hinzu."
            }
            "Nothing logged yet. Start the engine to see its UCI traffic." => {
                "Noch kein Protokoll. Starten Sie die Engine, um ihre UCI-Kommunikation zu sehen."
            }
            "Online" => "Online",
            "Open folder" => "Ordner öffnen",
            "Open in Analyse" => "In Analyse öffnen",
            "Open last game" => "Letzte Partie öffnen",
            "Open output window" => "Ausgabefenster öffnen",
            "Other" => "Sonstiges",
            "Pairing" => "Paarung",
            "Play line on board" => "Variante auf dem Brett spielen",
            "Play on board" => "Auf dem Brett spielen",
            "Add variation" => "Als Variante",
            "To repertoire" => "Ins Repertoire",
            "Copy line" => "Variante kopieren",
            "Click a move in the table to preview its line" => {
                "Einen Zug in der Tabelle anklicken, um seine Variante zu sehen"
            }
            "Not in repertoire" => "Nicht im Repertoire",
            "Restrict ›" => "Einschränken ›",
            "Hide" => "Ausblenden",
            "Choose the moves the engine searches from this position" => {
                "Züge wählen, die die Engine in dieser Stellung untersucht"
            }
            "Position note" => "Stellungsnotiz",
            "Add a note for this position" => "Notiz zu dieser Stellung",
            "Preview backup to restore" => "Sicherung zur Wiederherstellung ansehen",
            "Promote" => "Zur Hauptvariante machen",
            "Protocol" => "Protokoll",
            "Replace cached analysis" => "Gespeicherte Analyse ersetzen",
            "Search options" => "Suchoptionen",
            "Import a PGN file into the selected database" => {
                "Eine PGN-Datei in die gewählte Datenbank importieren"
            }
            "Restart engine" => "Engine neu starten",
            "Quit this engine and start it again" => "Diese Engine beenden und neu starten",
            "Restart" => "Neu starten",
            "Retry saving settings and repertoire" => {
                "Einstellungen und Repertoire erneut speichern"
            }
            "Review" => "Prüfung",
            "Review findings" => "Prüfergebnisse",
            "Engines at once" => "Engines gleichzeitig",
            "Review settings" => "Prüfeinstellungen",
            "Engine, limit and side for Analyse all" => {
                "Engine, Limit und Seite für Alle analysieren"
            }
            "Filtered games" => "Gefilterte Partien",
            "Selected game" => "Ausgewählte Partie",
            "Filtered player" => "Gefilterter Spieler",
            "Both" => "Beide",
            "Limit" => "Limit",
            "Side" => "Seite",
            "Choose an engine for batch review in Settings › Engines." => {
                "Wählen Sie unter Einstellungen › Engines eine Engine für die Stapelprüfung."
            }
            "Mate lost" => "Matt verpasst",
            "Run benchmark" => "Benchmark starten",
            "Save comment" => "Kommentar speichern",
            "Save current" => "Aktuelle speichern",
            "Scoresheet" => "Partieformular",
            "Value" => "Wert",
            "s" => "s",
            "Search limit" => "Suchlimit",
            "Search until stopped, instead of up to the search limit" => {
                "Suchen bis zum Anhalten statt bis zum Suchlimit"
            }
            "Sends debug on and shows the raw engine output in its own window" => {
                "Aktiviert den Debug-Modus und zeigt die Engine-Ausgabe in einem eigenen Fenster"
            }
            "Start fresh" => "Neu beginnen",
            "Start game" => "Partie starten",
            "Start tournament" => "Turnier starten",
            "Strength" => "Spielstärke",
            "Full strength" => "Volle Spielstärke",
            "Termination" => "Beendigung",
            "Tablebase" => "Endspieldatenbank",
            "Takeback" => "Zug zurücknehmen",
            "To move" => "Am Zug",
            "Used for" => "Verwendet für",
            "Waiting for WDL data" => "Warten auf WDL-Daten",
            "Watch all live" => "Alle live verfolgen",
            "Where you left the repertoire" => "Wo Sie vom Repertoire abgewichen sind",
            "to move" => "am Zug",
            "vs" => "gegen",
            "Delete from here" => "Ab hier löschen",
            "Delete all sidelines" => "Alle Nebenvarianten löschen",
            "Promote variation" => "Variante zur Hauptvariante machen",
            "Copy PGN from here" => "PGN ab hier kopieren",
            "PGN copied." => "PGN kopiert.",
            "FEN copied." => "FEN kopiert.",
            "White win" => "Sieg Weiß",
            "Black win" => "Sieg Schwarz",
            "Draw" => "Remis",
            "whole game" => "ganze Partie",
            "current position" => "aktuelle Stellung",
            "Start of game" => "Partieanfang",
            "End of line" => "Variantenende",
            "Open PGN or FEN" => "PGN oder FEN öffnen",
            "Save as PGN" => "Als PGN speichern",
            "Paste FEN or PGN" => "FEN oder PGN einfügen",
            "Engine on / off" => "Engine ein / aus",
            "Pause / resume" => "Pausieren / fortsetzen",
            "Analyse game backwards" => "Partie rückwärts analysieren",
            "More lines (MultiPV)" => "Mehr Varianten (MultiPV)",
            "Fewer lines (MultiPV)" => "Weniger Varianten (MultiPV)",
            "Engine output window" => "Engine-Ausgabefenster",
            "Analysis display" => "Analysedarstellung",
            "Sort by eval / depth" => "Nach Bewertung / Tiefe sortieren",
            "Next graph" => "Nächstes Diagramm",
            "Table / inline scoresheet" => "Partieformular als Tabelle / Fließtext",
            "Memory for the transposition table" => "Speicher für die Transpositionstabelle",
            "Lines shown in Moves from here" => "Angezeigte Varianten unter Züge ab hier",
            "Syzygy path" => "Syzygy-Pfad",
            "Endgame tablebases, 3–6 pieces" => "Endspieldatenbanken, 3–6 Figuren",
            "Nodes per position" => "Knoten pro Stellung",
            "A line for every legal move in every position (up to 256, as far as the engine allows)" => {
                "Eine Variante für jeden legalen Zug in jeder Stellung (bis zu 256, soweit die Engine dies unterstützt)"
            }
            "Open a new tab" => "Neuen Tab öffnen",
            "Rename tab" => "Tab umbenennen",
            "Double-click to rename" => "Doppelklick zum Umbenennen",
            "Create an empty database" => "Eine leere Datenbank erstellen",
            "Could not create the database" => "Die Datenbank konnte nicht erstellt werden",
            "Rename database" => "Datenbank umbenennen",
            "Duplicate database" => "Datenbank duplizieren",
            "copy" => "Kopie",
            "Edit game…" => "Partie bearbeiten…",
            "Edit game" => "Partie bearbeiten",
            "Delete game…" => "Partie löschen…",
            "Delete game" => "Partie löschen",
            "Delete games" => "Partien löschen",
            "Delete this game" => "Diese Partie löschen",
            "Edit players, result and other headers" => {
                "Spieler, Ergebnis und weitere Angaben bearbeiten"
            }
            "Saving in Analyse updates this game. Double-click a row to open it." => {
                "Speichern in Analyse aktualisiert diese Partie. Doppelklick auf eine Zeile öffnet sie."
            }
            "Their reviews go too. This cannot be undone." => {
                "Ihre Auswertungen werden ebenfalls gelöscht. Das lässt sich nicht rückgängig machen."
            }
            "Unsaved changes" => "Ungespeicherte Änderungen",
            "Hide the scoresheet" => "Partieformular ausblenden",
            "Show the scoresheet" => "Partieformular einblenden",
            "Saved in this database" => "In dieser Datenbank gespeichert",
            "Save moves, comments and variations to the database (Ctrl+S)" => {
                "Züge, Kommentare und Varianten in der Datenbank speichern (Strg+S)"
            }
            "engine line" => "Engine-Variante",
            "White rating" => "Elo Weiß",
            "Black rating" => "Elo Schwarz",
            "ECO" => "ECO",
            "An arrow per engine line, coloured from best (blue) to blunder (red)" => {
                "Ein Pfeil pro Engine-Variante, vom besten Zug (blau) bis zum groben Fehler (rot)"
            }
            "Another entry for this binary with its own options, e.g. Lc0 with another network" => {
                "Ein weiterer Eintrag für dieses Programm mit eigenen Optionen, z. B. Lc0 mit einem anderen Netz"
            }
            "Back to the selected engine's default number of lines" => {
                "Zur Standardanzahl der Varianten der ausgewählten Engine zurückkehren"
            }
            "Close tab; reopen with Ctrl/Cmd+Shift+T" => {
                "Tab schließen; mit Strg/Cmd+Umschalt+T wieder öffnen"
            }
            "Copy FEN or PGN" => "FEN oder PGN kopieren",
            "Copy the board as an image and save it" => "Brett als Bild kopieren und speichern",
            "Discard this saved result and run a fresh search" => {
                "Dieses gespeicherte Ergebnis verwerfen und neu suchen"
            }
            "Discard this session and start the queue again" => {
                "Diese Sitzung verwerfen und die Warteschlange neu starten"
            }
            "Edit position" => "Stellung bearbeiten",
            "Erase (or right-click a square)" => "Entfernen (oder ein Feld rechtsklicken)",
            "Every option this engine offers, in Settings › Engines" => {
                "Alle Optionen dieser Engine unter Einstellungen › Engines"
            }
            "Follow this engine's raw output in its own window" => {
                "Die Ausgabe dieser Engine in einem eigenen Fenster verfolgen"
            }
            "List every legal move, analysed or not" => {
                "Alle legalen Züge anzeigen, auch ohne Analyse"
            }
            "More actions" => "Weitere Aktionen",
            "MultiPV: lines the selected engine searches, saved in its options" => {
                "MultiPV: Varianten der ausgewählten Engine; in ihren Optionen gespeichert"
            }
            "Open the containing folder" => "Übergeordneten Ordner öffnen",
            "Click to open it in Analyse." => "Klicken, um ihn in Analyse zu öffnen.",
            "Order engine lines by evaluation or by search depth" => {
                "Engine-Varianten nach Bewertung oder Suchtiefe sortieren"
            }
            "Recommended: UHO_Lichess_4852_v1.epd. Stockfish books download on first use. Choose Custom file to use a local book." => {
                "Empfohlen: UHO_Lichess_4852_v1.epd. Stockfish-Bücher werden bei der ersten Verwendung heruntergeladen. Wählen Sie Eigene Datei für ein lokales Buch."
            }
            "Run another process of this engine as an opponent" => {
                "Einen weiteren Prozess dieser Engine als Gegner starten"
            }
            "Search one fewer engine line" => "Eine Engine-Variante weniger durchsuchen",
            "Show or hide engine output" => "Engine-Ausgabe anzeigen oder ausblenden",
            "Toggle this move in the engine's root search" => {
                "Diesen Zug in der Wurzelsuche der Engine ein- oder ausschließen"
            }
            "Win / draw / loss odds next to the eval" => {
                "Sieg-/Remis-/Niederlagechancen neben der Bewertung"
            }
            "Weights file" => "Gewichtedatei",
            "Show WDL" => "WDL anzeigen",
            "Lc0 network file" => "Lc0-Netzdatei",
            "Shown in tables and menus" => "In Tabellen und Menüs angezeigt",
            "Overrides move-quality colours for this engine" => {
                "Ersetzt die Zugqualitätsfarben für diese Engine"
            }
            "Preset option sets for this engine" => "Vordefinierte Optionssätze für diese Engine",
            "How you appear in PGN headers. Used for My games and leak detection." => {
                "Ihr Name in PGN-Partiedaten. Wird für Meine Partien und die Erkennung von Repertoireabweichungen verwendet."
            }
            "Arrow colour" => "Pfeilfarbe",
            "Auto" => "Automatisch",
            "Use the default rook" => "Standardturm verwenden",
            "Choose an icon" => "Symbol auswählen",
            "Debug" => "Debug",
            "Live analysis" => "Live-Analyse",
            "Batch game review" => "Stapelweise Partieprüfung",
            "Change" => "Ändern",
            "No default" => "Kein Standard",
            "Games, notes, repertoire and history" => "Partien, Notizen, Repertoire und Verlauf",
            "Time control" => "Bedenkzeit",
            "Colour" => "Farbe",
            "Random" => "Zufällig",
            "Start from" => "Ausgangsstellung",
            "Starting position" => "Grundstellung",
            "Current Analyse position" => "Aktuelle Analysestellung",
            "Repertoire line" => "Repertoirevariante",
            "Paste a FEN" => "FEN einfügen",
            "Paste a valid FEN to start from." => {
                "Fügen Sie eine gültige FEN als Ausgangsstellung ein."
            }
            "Hide evaluation during the game" => "Bewertung während der Partie ausblenden",
            "Allow premoves" => "Vorauszüge erlauben",
            "Allow takebacks" => "Zugrücknahmen erlauben",
            "No engine configured" => "Keine Engine eingerichtet",
            "Hint" => "Hinweis",
            "Show answer" => "Antwort anzeigen",
            "Why this move?" => "Warum dieser Zug?",
            "Skip" => "Überspringen",
            "Drill" => "Üben",
            "Ask this position again later" => "Diese Stellung später noch einmal abfragen",
            "Again" => "Noch einmal",
            "Known" => "Bekannt",
            "Learning" => "In Arbeit",
            "Due" => "Fällig",
            "New" => "Neu",
            "Nothing due" => "Nichts fällig",
            "Not in repertoire yet" => "Noch nicht im Repertoire",
            "No leaks found. Import your games to see where you leave the repertoire." => {
                "Keine Abweichungen gefunden. Importieren Sie Ihre Partien, um Repertoireabweichungen zu sehen."
            }
            "Mode: every prepared reply" => "Modus: jede vorbereitete Antwort",
            "Mode: any prepared reply" => "Modus: eine beliebige vorbereitete Antwort",
            "Answer shown. Grade how well you knew it." => {
                "Antwort angezeigt. Bewerten Sie, wie gut Sie sie kannten."
            }
            "No note saved for this move yet." => {
                "Für diesen Zug ist noch keine Notiz gespeichert."
            }
            "No leaks to drill." => "Keine Abweichungen zum Üben.",
            "This leak is already the next position in the session." => {
                "Diese Abweichung ist schon die nächste Stellung der Sitzung."
            }
            "The leak is now the next position to drill." => {
                "Die Abweichung ist jetzt die nächste Stellung zum Üben."
            }
            "Practise the repertoire move in each position above, next in this session" => {
                "Den Repertoirezug in jeder Stellung oben üben, als Nächstes in dieser Sitzung"
            }
            "No gaps: every line ends on an answered move." => {
                "Keine Lücken: Jede Variante endet mit einem beantworteten Zug."
            }
            "Add lines from PGN" => "Varianten aus PGN hinzufügen",
            "Backwards analysis finished." => "Rückwärtsanalyse abgeschlossen.",
            "Backwards analysis stopped." => "Rückwärtsanalyse angehalten.",
            "Backwards analysis stopped: no engine is running." => {
                "Rückwärtsanalyse angehalten: Keine Engine läuft."
            }
            "Benchmark needs a real engine; demo mode only has scripted lines." => {
                "Der Benchmark benötigt eine echte Engine; der Demomodus enthält nur vorgegebene Varianten."
            }
            "Beyond cp" => "Über cp",
            "Book order" => "Buchreihenfolge",
            "Custom book (.epd or .pgn)" => "Eigenes Buch (.epd oder .pgn)",
            "Delay between games (ms)" => "Pause zwischen Partien (ms)",
            "Downloads require an internet connection. Installed engines run offline." => {
                "Downloads benötigen eine Internetverbindung. Installierte Engines laufen offline."
            }
            "Drag across moves to highlight. Ctrl+C copies, Esc returns." => {
                "Zum Markieren über Züge ziehen. Strg+C kopiert, Esc kehrt zurück."
            }
            "Draw ratio" => "Remisquote",
            "Engine ready. Select it for analysis, review or play below." => {
                "Engine bereit. Wählen Sie unten Analyse, Prüfung oder Spielen aus."
            }
            "Extra fastchess options" => "Weitere fastchess-Optionen",
            "Filter by name or licence" => "Nach Name oder Lizenz filtern",
            "For moves in a row" => "Für aufeinanderfolgende Züge",
            "From move" => "Ab Zug",
            "Games per round" => "Partien pro Runde",
            "Hash (MB)" => "Hash (MB)",
            "Installed. Checking UCI handshake…" => "Installiert. UCI-Verbindung wird geprüft…",
            "Live boards are off because this command sets its own -events option." => {
                "Live-Bretter sind deaktiviert, weil dieser Befehl eine eigene -events-Option setzt."
            }
            "Live boards need a fastchess build with the -events option." => {
                "Live-Bretter benötigen eine fastchess-Version mit der Option -events."
            }
            "Maximum moves" => "Maximale Zugzahl",
            "No engines yet. Add them in Settings › Engines, or add an executable here." => {
                "Noch keine Engines. Fügen Sie unter Einstellungen › Engines oder hier ein Programm hinzu."
            }
            "Opening book" => "Eröffnungsbuch",
            "Opp. rating ≥" => "Gegnerwertung ≥",
            "Own executable (optional)" => "Eigenes Programm (optional)",
            "Pairs ratio" => "Paarquote",
            "Paste a FEN and press Enter" => "FEN einfügen und Eingabetaste drücken",
            "Plies from book" => "Halbzüge aus dem Buch",
            "Profile name, e.g. BT2" => "Profilname, z. B. BT2",
            "Random seed" => "Zufallsstartwert",
            "Run a command line in folder (for its relative paths)" => {
                "Befehl in einem Ordner ausführen (für relative Pfade)"
            }
            "Running benchmark…" => "Benchmark läuft…",
            "Search positions, games, openings" => "Stellungen, Partien, Eröffnungen suchen",
            "Time margin (ms)" => "Zeitreserve (ms)",
            "WL/DD ratio" => "Sieg/Niederlage-zu-Remis/Remis-Verhältnis",
            "Within ± cp" => "Innerhalb ± cp",
            "Crate" => "Crate",
            "Version" => "Version",
            "Each analysis engine searches this many nodes in every position, from the last move back to the first. A fixed node count gives every position the same effort on any machine." => {
                "Jede Analyse-Engine durchsucht so viele Knoten pro Stellung, vom letzten Zug zurück zum ersten. Eine feste Knotenzahl gewährleistet für jede Stellung auf jedem Computer denselben Suchaufwand."
            }
            "Start it with Analyse backwards above the game graph, or its shortcut. The search limit for ordinary analysis is not used." => {
                "Starten Sie die Analyse mit Rückwärts analysieren über dem Partiediagramm oder dem zugehörigen Tastenkürzel. Das Suchlimit der normalen Analyse wird dabei nicht verwendet."
            }
            "Fast has no accounts or cloud sync: everything stays on this computer. Only Analyse's Online panel, while open, sends the position on the board to chesstb-explorer.hf.space and chessdb.cn." => {
                "Fast hat keine Konten oder Cloud-Synchronisierung: Alles bleibt auf diesem Computer. Nur das geöffnete Online-Panel in Analyse sendet die Brettstellung an chesstb-explorer.hf.space und chessdb.cn."
            }
            "Arrows" => "Pfeile",
            "Resume search (P)" => "Suche fortsetzen (P)",
            "Pause search and keep the current lines (P)" => {
                "Suche pausieren und aktuelle Varianten behalten (P)"
            }
            "Start analysis" => "Analyse starten",
            "Search moves" => "Suchzüge",
            "all" => "alle",
            "selected" => "ausgewählt",
            "games" => "Partien",
            "Speed" => "Tempo",
            "Seed" => "Gesetzt",
            "No engine" => "Keine Engine",
            "Selection copied." => "Auswahl kopiert.",
            "Log copied." => "Protokoll kopiert.",
            "Line copied." => "Variante kopiert.",
            "Info" => "Info",
            "Not in the tablebase" => "Nicht in der Endspieldatenbank",
            "No moves" => "Keine Züge",
            "Not analysed yet" => "Noch nicht analysiert",
            "Mate" => "Matt",
            "Looking up…" => "Abfrage läuft…",
            "Too many pieces for the tablebase" => "Zu viele Figuren für die Endspieldatenbank",
            "Slate" => "Schiefer",
            "Paper" => "Papier",
            "Sand" => "Sand",
            "Forest" => "Wald",
            "Plum" => "Pflaume",
            "Obsidian" => "Obsidian",
            "Blue" => "Blau",
            "Walnut" => "Walnuss",
            "Moss" => "Moos",
            "Graphite" => "Graphit",
            "Classic" => "Klassisch",
            "Fantasy" => "Fantasie",
            "Letters" => "Buchstaben",
            "Flat" => "Flach",
            "Show details" => "Details anzeigen",
            "Import cancelled after {} games." => "Import nach {} Partien abgebrochen.",
            "Import failed:" => "Import fehlgeschlagen:",
            "Imported {} games, {} with annotations." => {
                "{} Partien importiert, {} mit Anmerkungen."
            }
            "{} games skipped." => "{} Partien übersprungen.",
            "{} variations had illegal moves and were left out." => {
                "{} Varianten enthielten unzulässige Züge und wurden ausgelassen."
            }
            "Into" => "In",
            "Keep comments and variations" => "Kommentare und Varianten behalten",
            "Turn this off for very large reference files where size matters." => {
                "Für sehr große Referenzdateien ausschalten, wenn die Größe zählt."
            }
            "Choose PGN file…" => "PGN-Datei wählen…",
            "Import games" => "Partien importieren",
            "These games kept their moves, but a variation with an illegal move was left out. Numbers count games in the file from 1." => {
                "Diese Partien behielten ihre Züge, aber eine Variante mit einem unzulässigen Zug wurde ausgelassen. Die Nummern zählen die Partien in der Datei ab 1."
            }
            "and {} more" => "und {} weitere",
            "Variations left out" => "Ausgelassene Varianten",
            "Has comments or variations" => "Hat Kommentare oder Varianten",
            "Replace the games in {} with {}?" => "Partien in {} durch {} ersetzen?",
            "Edits and reviews of these games are lost. This cannot be undone." => {
                "Änderungen und Bewertungen dieser Partien gehen verloren. Dies kann nicht rückgängig gemacht werden."
            }
            "Re-import" => "Neu importieren",
            "Re-import from file" => "Aus Datei neu importieren",
            "Re-import from file…" => "Aus Datei neu importieren…",
            "Opening tree now uses {}" => "Eröffnungsbaum nutzt jetzt {}",
            "Show these games in the Database tab" => "Diese Partien im Datenbank-Tab zeigen",
            "After {}" => "Nach {}",
            "No games reach this position" => "Keine Partie erreicht diese Stellung",
            "Choose the database for the opening tree" => "Datenbank für den Eröffnungsbaum wählen",
            "This game's database" => "Datenbank dieser Partie",
            "Next opening-tree database" => "Nächste Datenbank für den Eröffnungsbaum",
            "Reference" => "Referenz",
            "No {} user named '{}'" => "Kein {}-Konto namens '{}'",
            "You're offline. Try again later." => {
                "Keine Verbindung. Bitte später erneut versuchen."
            }
            "{} asks us to wait. Try again in a few minutes." => {
                "{} bittet zu warten. In ein paar Minuten erneut versuchen."
            }
            "The connection broke after {} games. Update to fetch the rest." => {
                "Die Verbindung brach nach {} Partien ab. Aktualisieren lädt den Rest."
            }
            "The site answered with error {}. Try again later." => {
                "Die Seite antwortete mit Fehler {}. Bitte später erneut versuchen."
            }
            "Could not store the games: {}" => "Partien konnten nicht gespeichert werden: {}",
            "Could not read the site's answer: {}" => "Antwort der Seite nicht lesbar: {}",
            "Type a username first." => "Zuerst einen Benutzernamen eingeben.",
            "Downloaded {} new games ({} already in {})" => {
                "{} neue Partien geladen ({} schon in {})"
            }
            "Show games" => "Partien zeigen",
            "{} asks us to wait. Retrying in {} s." => {
                "{} bittet zu warten. Neuer Versuch in {} s."
            }
            "Downloading {} from {}: {} games" => "Lade {} von {}: {} Partien",
            "Username" => "Benutzername",
            "PGN file" => "PGN-Datei",
            "Last year" => "Letztes Jahr",
            "Last month" => "Letzter Monat",
            "Since last update" => "Seit letzter Aktualisierung",
            "Since" => "Seit",
            "Include clocks and evaluations" => "Uhrzeiten und Bewertungen einschließen",
            "This is my account" => "Das ist mein Konto",
            "Fast connects to {} to fetch the games. Nothing else is sent." => {
                "Fast verbindet sich mit {}, um die Partien zu laden. Sonst wird nichts gesendet."
            }
            "The download keeps running" => "Der Download läuft weiter",
            "Download games" => "Partien laden",
            "{} of {} months" => "{} von {} Monaten",
            "Downloading {}…" => "Lade {}…",
            "Already stored" => "Schon gespeichert",
            "Update {} from {}" => "{} von {} aktualisieren",
            "Update" => "Aktualisieren",
            "Forget" => "Vergessen",
            "No accounts yet. Use Import › Lichess or Chess.com on the Database screen." => {
                "Noch keine Konten. Im Datenbank-Tab Import › Lichess oder Chess.com verwenden."
            }
            "Updates only run when you ask. Each one fetches the games played since the last." => {
                "Aktualisierungen laufen nur auf Wunsch. Jede lädt die Partien seit der letzten."
            }
            "Win" => "Sieg",
            "Loss" => "Niederlage",
            "From" => "Von",
            "To" => "Bis",
            "Event or site" => "Turnier oder Ort",
            "Min" => "Min",
            "Max" => "Max",
            "Name of the search" => "Name der Suche",
            "Use 2020, 2020.05 or 2020.05.17" => "2020, 2020.05 oder 2020.05.17 verwenden",
            "Delete saved search" => "Gespeicherte Suche löschen",
            "Right-click to delete" => "Rechtsklick zum Löschen",
            "Only annotated games" => "Nur kommentierte Partien",
            "Import a PGN file, or download games from Lichess or Chess.com" => {
                "PGN-Datei importieren oder Partien von Lichess oder Chess.com laden"
            }
            "Save search" => "Suche speichern",
            "Save these filters under a name" => "Diese Filter unter einem Namen speichern",
            "More filters" => "Weitere Filter",
            "{} of {} games" => "{} von {} Partien",
            "{} games" => "{} Partien",
            "Clear filters" => "Filter löschen",
            _ => text,
        }
    } else {
        text
    }
}

pub fn t(text: &str) -> &str {
    let language = if LANGUAGE.load(Ordering::Relaxed) == 1 {
        Language::German
    } else {
        Language::English
    };
    translate(language, text)
}

/// Translates a message with `{}` placeholders, then fills them with `args` in order,
/// so formatted messages such as "Imported {} games." are translated too.
pub fn tf(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut parts = t(template).split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (index, part) in parts.enumerate() {
        out.push_str(args.get(index).copied().unwrap_or(""));
        out.push_str(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translations_and_fallback() {
        assert_eq!(translate(Language::German, "Settings"), "Einstellungen");
        assert_eq!(translate(Language::English, "Settings"), "Settings");
        assert_eq!(
            translate(Language::German, "Unknown message"),
            "Unknown message"
        );
    }

    #[test]
    fn templates_fill_placeholders_in_order() {
        assert_eq!(tf("{} of {} games", &["3", "10"]), "3 of 10 games");
        assert_eq!(tf("No placeholders", &["x"]), "No placeholders");
    }
}
