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
            "Analyse" => "Analyse",
            "Database" => "Datenbank",
            "Train" => "Training",
            "Play" => "Spielen",
            "Settings" => "Einstellungen",
            "Tournament" => "Turnier",
            "General" => "Allgemein",
            "Board" => "Brett",
            "Board & pieces" => "Brett und Figuren",
            "Engines" => "Engines",
            "Databases" => "Datenbanken",
            "Keyboard shortcuts" => "Tastenkürzel",
            "Shortcuts" => "Tastenkürzel",
            "Accounts" => "Konten",
            "Licenses" => "Lizenzen",
            "Licences" => "Lizenzen",
            "Appearance" => "Darstellung",
            "Theme" => "Design",
            "Language" => "Sprache",
            "Applies to the entire interface" => "Gilt für die gesamte Oberfläche",
            "You" => "Spieler",
            "Files" => "Dateien",
            "Player name" => "Spielername",
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
            "Unlimited" => "Unbegrenzt",
            "Infinite" => "Unbegrenzt",
            "Seconds" => "Sekunden",
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
            "Auto queen" => "Automatisch zur Dame umwandeln",
            "Automatic queen promotion" => "Automatische Umwandlung zur Dame",
            "On squares" => "Auf den Feldern",
            "Outside" => "Außerhalb",
            "Table" => "Tabelle",
            "Inline" => "Fließtext",
            "All moves" => "Alle Züge",
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
            "Download & install" => "Herunterladen und installieren",
            "Add from file" => "Aus Datei hinzufügen",
            "Source" => "Quelle",
            "None yet" => "Noch keine",
            "Use" => "Verwenden",
            "Download now" => "Jetzt herunterladen",
            "Cancel download" => "Download abbrechen",
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
            "Analyse backwards" => "Rückwärts analysieren",
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
}
