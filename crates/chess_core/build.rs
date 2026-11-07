use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() {
    let mut openings = BTreeMap::new();
    for volume in ['a', 'b', 'c', 'd', 'e'] {
        let path = format!("data/chess-openings/{volume}.tsv");
        println!("cargo:rerun-if-changed={path}");
        let data = fs::read_to_string(&path).expect("read bundled ECO data");
        let mut lines = data.lines();
        assert_eq!(
            lines.next(),
            Some("eco\tname\tpgn"),
            "{path}: invalid header"
        );
        for row in lines {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), 3, "{path}: invalid row: {row}");
            let (eco, name, pgn) = (fields[0], fields[1], fields[2]);
            assert!(
                eco.len() == 3
                    && eco.starts_with(volume.to_ascii_uppercase())
                    && eco.as_bytes()[1..].iter().all(u8::is_ascii_digit)
            );
            assert!(!name.is_empty());
            let moves = pgn
                .split_whitespace()
                .filter(|token| !token.ends_with('.'))
                .map(|san| san.trim_end_matches(['+', '#']))
                .collect::<Vec<_>>()
                .join(" ");
            assert!(!moves.is_empty());
            assert!(
                openings
                    .insert(moves, (eco.to_owned(), name.to_owned()))
                    .is_none(),
                "{path}: duplicate opening line: {pgn}"
            );
        }
    }
    let mut generated = String::from("const OPENINGS: &[(&str, &str, &str)] = &[\n");
    for (moves, (eco, name)) in openings {
        generated.push_str(&format!("    ({moves:?}, {eco:?}, {name:?}),\n"));
    }
    generated.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    fs::write(out.join("eco_table.rs"), generated).expect("write compiled ECO table");
}
