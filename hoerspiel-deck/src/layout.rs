//! Which tracks exist and what each Stream Deck key does.

/// Tracks created in Audacity, top to bottom.
pub const TRACKS: [&str; 12] = [
    "Erzähler",
    "Hinnerk",
    "Jonas",
    "Birte",
    "Ole",
    "Klaas",
    "Nordic Star",
    "Bremen Rescue",
    "Funk Hinnerk",
    "Funk Klaas",
    "Funk Nordic Star",
    "Funk Bremen Rescue",
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// Select the track with this name.
    Track(&'static str),
    /// Start recording, or stop it if already recording.
    Record,
    Stop,
    Undo,
}

pub struct Key {
    pub action: Action,
    /// PNG embedded into the binary at build time.
    pub icon: &'static [u8],
}

/// Number of columns of the layout below (Stream Deck MK.2 / Original: 5 x 3).
/// Decks of another size get this grid placed in their top left corner.
pub const COLUMNS: usize = 5;

/// Keys row by row, left to right.
pub const LAYOUT: [Key; 15] = [
    // Row 1: crew of the Seeadler and the narrator
    key(
        Action::Track("Erzähler"),
        include_bytes!("../assets/01_erzaehler.png"),
    ),
    key(
        Action::Track("Hinnerk"),
        include_bytes!("../assets/02_hinnerk.png"),
    ),
    key(
        Action::Track("Jonas"),
        include_bytes!("../assets/03_jonas.png"),
    ),
    key(
        Action::Track("Birte"),
        include_bytes!("../assets/04_birte.png"),
    ),
    key(Action::Track("Ole"), include_bytes!("../assets/05_ole.png")),
    // Row 2: other characters, transport
    key(
        Action::Track("Klaas"),
        include_bytes!("../assets/06_klaas.png"),
    ),
    key(
        Action::Track("Nordic Star"),
        include_bytes!("../assets/07_nordic_star.png"),
    ),
    key(
        Action::Track("Bremen Rescue"),
        include_bytes!("../assets/08_bremen_rescue.png"),
    ),
    key(Action::Record, include_bytes!("../assets/13_aufnahme.png")),
    key(Action::Stop, include_bytes!("../assets/14_stopp.png")),
    // Row 3: radio tracks, undo
    key(
        Action::Track("Funk Hinnerk"),
        include_bytes!("../assets/09_funk_hinnerk.png"),
    ),
    key(
        Action::Track("Funk Klaas"),
        include_bytes!("../assets/10_funk_klaas.png"),
    ),
    key(
        Action::Track("Funk Nordic Star"),
        include_bytes!("../assets/11_funk_nordic_star.png"),
    ),
    key(
        Action::Track("Funk Bremen Rescue"),
        include_bytes!("../assets/12_funk_bremen_rescue.png"),
    ),
    key(
        Action::Undo,
        include_bytes!("../assets/15_rueckgaengig.png"),
    ),
];

const fn key(action: Action, icon: &'static [u8]) -> Key {
    Key { action, icon }
}

/// Maps each physical key of a deck (row-major index) to an entry of [`LAYOUT`], if any.
pub fn map_keys(deck_columns: usize, deck_keys: usize) -> Vec<Option<usize>> {
    (0..deck_keys)
        .map(|key| {
            let (row, column) = (key / deck_columns, key % deck_columns);
            if column >= COLUMNS {
                return None;
            }
            let index = row * COLUMNS + column;
            (index < LAYOUT.len()).then_some(index)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_track_key_has_a_track() {
        for k in &LAYOUT {
            if let Action::Track(name) = k.action {
                assert!(TRACKS.contains(&name), "{name} missing in TRACKS");
            }
        }
    }

    #[test]
    fn mk2_maps_one_to_one() {
        let map = map_keys(5, 15);
        assert_eq!(map, (0..15).map(Some).collect::<Vec<_>>());
    }

    #[test]
    fn xl_uses_top_left_corner() {
        let map = map_keys(8, 32);
        assert_eq!(map[0], Some(0));
        assert_eq!(map[5], None); // column 5 is outside the 5-column layout
        assert_eq!(map[8], Some(5)); // second row starts with layout index 5
        assert_eq!(map[24], None); // fourth row is unused
    }

    #[test]
    fn icons_decode() {
        for k in &LAYOUT {
            image::load_from_memory(k.icon).expect("embedded icon must be a valid PNG");
        }
    }
}
