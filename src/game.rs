use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub const BOARD_SIZES: [u32; 9] = [1, 2, 4, 8, 16, 24, 32, 48, 64];
pub const COLORS: [&str; 12] = [
    "#111318", "#f5f1e8", "#ff5d73", "#ff9f43", "#ffd93d", "#6bcb77", "#4d96ff", "#845ec2",
    "#d65db1", "#00c9a7", "#8f5a3c", "#8b95a5",
];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    pub x: u32,
    pub y: u32,
    pub color: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct Pixel {
    pub x: u32,
    pub y: u32,
    pub color: u8,
    pub owner: String,
    pub received_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Activity {
    pub id: u64,
    pub kind: String,
    pub message: String,
    pub at: u64,
}

#[derive(Debug)]
pub enum ApplyResult {
    Applied { resized_to: Option<u32> },
    Duplicate,
    Rejected(String),
}

#[derive(Debug, Serialize)]
pub struct GameSnapshot {
    pub size: u32,
    pub next_size: Option<u32>,
    pub filled: usize,
    pub total: u32,
    pub processed_moves: u64,
    pub pixels: Vec<Pixel>,
    pub activity: Vec<Activity>,
    pub sizes: Vec<u32>,
    pub colors: Vec<&'static str>,
}

pub struct Game {
    size_index: usize,
    pixels: HashMap<(u32, u32), Pixel>,
    seen_resources: HashSet<String>,
    processed_moves: u64,
    activity_sequence: u64,
    activity: VecDeque<Activity>,
}

impl Game {
    pub fn new() -> Self {
        let mut game = Self {
            size_index: 0,
            pixels: HashMap::new(),
            seen_resources: HashSet::new(),
            processed_moves: 0,
            activity_sequence: 0,
            activity: VecDeque::new(),
        };
        game.note(
            "watcher",
            "Waiting for a Pubky Ring-authenticated player to register",
        );
        game
    }

    pub fn apply_move(&mut self, resource: &str, owner: &str, next: Move) -> ApplyResult {
        if self.seen_resources.contains(resource) {
            return ApplyResult::Duplicate;
        }

        let size = self.size();
        if next.x >= size || next.y >= size {
            return ApplyResult::Rejected(format!(
                "ignored out-of-bounds move ({}, {}) for {size}x{size}",
                next.x, next.y
            ));
        }
        if usize::from(next.color) >= COLORS.len() {
            return ApplyResult::Rejected(format!(
                "ignored color {} (palette has {} colors)",
                next.color,
                COLORS.len()
            ));
        }

        self.seen_resources.insert(resource.to_owned());
        self.processed_moves += 1;
        self.pixels.insert(
            (next.x, next.y),
            Pixel {
                x: next.x,
                y: next.y,
                color: next.color,
                owner: owner.to_owned(),
                received_at: unix_millis(),
            },
        );
        self.note(
            "move",
            format!("{} painted ({}, {})", short_key(owner), next.x, next.y),
        );

        let resized_to = if self.is_full() && self.size_index + 1 < BOARD_SIZES.len() {
            self.size_index += 1;
            let new_size = self.size();
            self.note("resize", format!("Canvas unlocked {new_size}x{new_size}"));
            Some(new_size)
        } else {
            None
        };

        ApplyResult::Applied { resized_to }
    }

    pub fn note(&mut self, kind: impl Into<String>, message: impl Into<String>) {
        self.activity_sequence += 1;
        self.activity.push_front(Activity {
            id: self.activity_sequence,
            kind: kind.into(),
            message: message.into(),
            at: unix_millis(),
        });
        self.activity.truncate(30);
    }

    pub fn snapshot(&self) -> GameSnapshot {
        let mut pixels: Vec<_> = self.pixels.values().cloned().collect();
        pixels.sort_by_key(|pixel| (pixel.y, pixel.x));
        GameSnapshot {
            size: self.size(),
            next_size: BOARD_SIZES.get(self.size_index + 1).copied(),
            filled: self.pixels.len(),
            total: self.size() * self.size(),
            processed_moves: self.processed_moves,
            pixels,
            activity: self.activity.iter().cloned().collect(),
            sizes: BOARD_SIZES.to_vec(),
            colors: COLORS.to_vec(),
        }
    }

    fn size(&self) -> u32 {
        BOARD_SIZES[self.size_index]
    }

    fn is_full(&self) -> bool {
        self.pixels.len() as u32 == self.size() * self.size()
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn short_key(key: &str) -> String {
    if key.len() <= 14 {
        return key.to_owned();
    }
    format!("{}…{}", &key[..8], &key[key.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::{ApplyResult, Game, Move, BOARD_SIZES};

    #[test]
    fn follows_the_requested_resize_sequence() {
        let mut game = Game::new();
        let mut resource = 0;

        for expected in BOARD_SIZES.iter().copied().skip(1) {
            let size = game.size();
            for y in 0..size {
                for x in 0..size {
                    if game.pixels.contains_key(&(x, y)) {
                        continue;
                    }
                    resource += 1;
                    game.apply_move(
                        &format!("pubky://alice/pub/pubky-watcher-canvas/moves/{resource}"),
                        "alice",
                        Move { x, y, color: 1 },
                    );
                }
            }
            assert_eq!(game.size(), expected);
        }

        assert_eq!(game.size(), 64);
        for y in 0..64 {
            for x in 0..64 {
                if game.pixels.contains_key(&(x, y)) {
                    continue;
                }
                resource += 1;
                game.apply_move(
                    &format!("pubky://alice/pub/pubky-watcher-canvas/moves/{resource}"),
                    "alice",
                    Move { x, y, color: 1 },
                );
            }
        }
        assert_eq!(game.size(), 64);
        assert_eq!(game.snapshot().next_size, None);
    }

    #[test]
    fn rejects_moves_outside_the_current_stage() {
        let mut game = Game::new();
        let result = game.apply_move(
            "pubky://alice/pub/pubky-watcher-canvas/moves/1",
            "alice",
            Move {
                x: 1,
                y: 0,
                color: 2,
            },
        );
        assert!(matches!(result, ApplyResult::Rejected(_)));
        assert_eq!(game.snapshot().processed_moves, 0);
    }
}
