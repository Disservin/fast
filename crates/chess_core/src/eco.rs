//! Bundled Lichess opening data. Longest matching SAN prefix wins.

/// A named opening with its ECO code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub eco: String,
    pub name: String,
}

include!(concat!(env!("OUT_DIR"), "/eco_table.rs"));

static UPDATED: std::sync::RwLock<Option<OpeningTable>> = std::sync::RwLock::new(None);

/// A complete, validated replacement for the bundled data.
pub struct OpeningTable {
    entries: std::collections::BTreeMap<String, Opening>,
}

impl OpeningTable {
    pub fn parse(volumes: &[String; 5]) -> Result<Self, String> {
        let mut entries = std::collections::BTreeMap::new();
        let mut codes = std::collections::BTreeSet::new();
        for (volume, text) in ('A'..='E').zip(volumes) {
            if text.len() > 4 * 1024 * 1024 {
                return Err("Opening file exceeds size limit".into());
            }
            let mut lines = text.lines();
            if lines.next() != Some("eco\tname\tpgn") {
                return Err(format!("Invalid header in volume {volume}"));
            }
            for row in lines {
                let fields: Vec<_> = row.split('\t').collect();
                if fields.len() != 3 {
                    return Err(format!("Invalid row in volume {volume}"));
                }
                let (eco, name, pgn) = (fields[0], fields[1], fields[2]);
                if eco.len() != 3
                    || !eco.starts_with(volume)
                    || !eco.as_bytes()[1..].iter().all(u8::is_ascii_digit)
                    || name.is_empty()
                    || name.chars().any(char::is_control)
                {
                    return Err(format!("Invalid opening in volume {volume}"));
                }
                let moves: Vec<_> = pgn
                    .split_whitespace()
                    .filter(|s| !s.ends_with('.'))
                    .collect();
                if moves.is_empty()
                    || moves.iter().any(|san| {
                        shakmaty::san::San::from_ascii(san.trim_end_matches(['+', '#']).as_bytes())
                            .is_err()
                    })
                {
                    return Err(format!("Invalid moves for {name}"));
                }
                let line = moves
                    .iter()
                    .map(|s| s.trim_end_matches(['+', '#']))
                    .collect::<Vec<_>>()
                    .join(" ");
                if entries
                    .insert(
                        line,
                        Opening {
                            eco: eco.into(),
                            name: name.into(),
                        },
                    )
                    .is_some()
                {
                    return Err("Duplicate opening line".into());
                }
                codes.insert(eco);
            }
        }
        if codes.len() != 500 {
            return Err("Opening update must contain all 500 ECO codes".into());
        }
        Ok(Self { entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn activate(self) {
        *UPDATED.write().unwrap_or_else(|e| e.into_inner()) = Some(self);
    }
}

/// The most specific opening matching a sequence of SAN moves, ignoring check marks.
pub fn classify<'a>(moves: impl IntoIterator<Item = &'a str>) -> Option<Opening> {
    let active = UPDATED.read().unwrap_or_else(|e| e.into_inner());
    let mut line = moves
        .into_iter()
        .map(|san| san.trim_end_matches(['+', '#']))
        .collect::<Vec<_>>()
        .join(" ");
    loop {
        if let Some(table) = active.as_ref() {
            if let Some(opening) = table.entries.get(&line) {
                return Some(opening.clone());
            }
        } else if let Ok(index) = OPENINGS.binary_search_by_key(&line.as_str(), |entry| entry.0) {
            let (_, eco, name) = OPENINGS[index];
            return Some(Opening {
                eco: eco.into(),
                name: name.into(),
            });
        }
        let end = line.rfind(' ')?;
        line.truncate(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_table_validates_complete_data_and_rejects_bad_updates() {
        let volumes = [
            include_str!("../data/chess-openings/a.tsv").to_owned(),
            include_str!("../data/chess-openings/b.tsv").to_owned(),
            include_str!("../data/chess-openings/c.tsv").to_owned(),
            include_str!("../data/chess-openings/d.tsv").to_owned(),
            include_str!("../data/chess-openings/e.tsv").to_owned(),
        ];
        let table = OpeningTable::parse(&volumes).unwrap();
        assert_eq!(table.len(), OPENINGS.len());
        for &(line, eco, name) in OPENINGS {
            assert_eq!(
                table.entries[line],
                Opening {
                    eco: eco.into(),
                    name: name.into()
                }
            );
        }
        let mut invalid = volumes.clone();
        invalid[0].push_str("A00\tInvalid\t1. nonsense\n");
        assert!(OpeningTable::parse(&invalid).is_err());
        let mut duplicate = volumes.clone();
        duplicate[0].push_str("A00\tDuplicate\t1. Nh3\n");
        assert!(OpeningTable::parse(&duplicate).is_err());
        let mut incomplete = volumes;
        incomplete[4] = "eco\tname\tpgn\n".into();
        assert!(OpeningTable::parse(&incomplete).is_err());
    }

    #[test]
    fn all_upstream_lines_and_eco_codes_are_available() {
        let mut codes = std::collections::BTreeSet::new();
        let mut count = 0;
        for data in [
            include_str!("../data/chess-openings/a.tsv"),
            include_str!("../data/chess-openings/b.tsv"),
            include_str!("../data/chess-openings/c.tsv"),
            include_str!("../data/chess-openings/d.tsv"),
            include_str!("../data/chess-openings/e.tsv"),
        ] {
            for row in data.lines().skip(1) {
                let fields: Vec<_> = row.split('\t').collect();
                let moves = fields[2].split_whitespace().filter(|s| !s.ends_with('.'));
                let expected = Opening {
                    eco: fields[0].into(),
                    name: fields[1].into(),
                };
                assert_eq!(
                    classify(moves.clone()),
                    Some(expected.clone()),
                    "{}",
                    fields[2]
                );
                let marked: Vec<_> = moves.map(|san| format!("{san}+#")).collect();
                assert_eq!(classify(marked.iter().map(String::as_str)), Some(expected));
                codes.insert(fields[0]);
                count += 1;
            }
        }
        assert_eq!(count, OPENINGS.len());
        for volume in ['A', 'B', 'C', 'D', 'E'] {
            for number in 0..100 {
                assert!(codes.contains(format!("{volume}{number:02}").as_str()));
            }
        }
    }

    #[test]
    fn longest_prefix_wins() {
        let najdorf = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3";
        let opening = classify(najdorf.split(' ')).unwrap();
        assert_eq!(opening.eco, "B90");
        assert_eq!(
            opening.name,
            "Sicilian Defense: Najdorf Variation, English Attack"
        );
        assert_eq!(
            classify("e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6".split(' '))
                .unwrap()
                .eco,
            "C54"
        );
        assert_eq!(classify(["a4"]).unwrap().eco, "A00");
        assert!(classify([]).is_none());
        assert!(classify(["e4x"]).is_none());
    }
}
