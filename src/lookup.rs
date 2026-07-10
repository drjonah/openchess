//! Precomputed attack tables and magic bitboard slider lookups.
//!
//! Call [`initialize()`] once at process startup (for example from `main`).
//! Tests also call it in setup. All public query functions invoke initialization
//! lazily if it has not run yet.

use crate::types::{Bitboard, Color, PieceType, Square};
use std::sync::OnceLock;

const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (-1, 1), (1, -1), (-1, -1)];
const ROOK_DIRS: [(i8, i8); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];
const KNIGHT_DELTAS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING_DELTAS: [(i8, i8); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

struct MagicEntry {
    mask: Bitboard,
    magic: u64,
    shift: u8,
    offset: usize,
}

struct LookupTables {
    knight_attacks: [Bitboard; 64],
    king_attacks: [Bitboard; 64],
    pawn_attacks: [[Bitboard; 64]; 2],
    bishop_magics: [MagicEntry; 64],
    rook_magics: [MagicEntry; 64],
    bishop_attacks: Vec<Bitboard>,
    rook_attacks: Vec<Bitboard>,
}

static TABLES: OnceLock<LookupTables> = OnceLock::new();

/// Build attack tables once. Safe to call multiple times.
pub fn initialize() {
    let _ = TABLES.get_or_init(build_tables);
}

fn tables() -> &'static LookupTables {
    initialize();
    TABLES.get().expect("lookup tables initialized")
}

fn build_tables() -> LookupTables {
    let mut knight_attacks = [Bitboard::EMPTY; 64];
    let mut king_attacks = [Bitboard::EMPTY; 64];
    let mut pawn_attacks = [[Bitboard::EMPTY; 64]; 2];

    for sq in Square::all() {
        let idx = sq.index() as usize;
        knight_attacks[idx] = init_knight_attacks(sq);
        king_attacks[idx] = init_king_attacks(sq);
        pawn_attacks[Color::White.index()][idx] = init_pawn_attacks(Color::White, sq);
        pawn_attacks[Color::Black.index()][idx] = init_pawn_attacks(Color::Black, sq);
    }

    let (bishop_magics, bishop_attacks) = init_slider_magics(true);
    let (rook_magics, rook_attacks) = init_slider_magics(false);

    LookupTables {
        knight_attacks,
        king_attacks,
        pawn_attacks,
        bishop_magics,
        rook_magics,
        bishop_attacks,
        rook_attacks,
    }
}

fn init_knight_attacks(sq: Square) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(df, dr) in &KNIGHT_DELTAS {
        if let Some(target) = sq.offset(df, dr) {
            attacks = attacks.with(target);
        }
    }
    attacks
}

fn init_king_attacks(sq: Square) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(df, dr) in &KING_DELTAS {
        if let Some(target) = sq.offset(df, dr) {
            attacks = attacks.with(target);
        }
    }
    attacks
}

fn init_pawn_attacks(color: Color, sq: Square) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    let (df_left, df_right, dr) = match color {
        Color::White => (-1, 1, 1),
        Color::Black => (-1, 1, -1),
    };
    if let Some(left) = sq.offset(df_left, dr) {
        attacks = attacks.with(left);
    }
    if let Some(right) = sq.offset(df_right, dr) {
        attacks = attacks.with(right);
    }
    attacks
}

fn slider_attacks(sq: Square, occ: Bitboard, dirs: &[(i8, i8)]) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(df, dr) in dirs {
        let mut current = sq;
        loop {
            let Some(next) = current.offset(df, dr) else {
                break;
            };
            attacks = attacks.with(next);
            if occ.contains(next) {
                break;
            }
            current = next;
        }
    }
    attacks
}

fn bishop_attacks_slow(sq: Square, occ: Bitboard) -> Bitboard {
    slider_attacks(sq, occ, &BISHOP_DIRS)
}

fn rook_attacks_slow(sq: Square, occ: Bitboard) -> Bitboard {
    slider_attacks(sq, occ, &ROOK_DIRS)
}

fn slider_mask(sq: Square, dirs: &[(i8, i8)]) -> Bitboard {
    // Magic blocker masks omit the board-edge square in each ray: only interior
    // blockers affect the index. Rays that already lie on an edge file/rank
    // (e.g. rook on a1 looking north) must still include a2..a7 — do not treat
    // "currently on file A" as a reason to stop.
    let mut mask = Bitboard::EMPTY;
    for &(df, dr) in dirs {
        let mut current = sq;
        loop {
            let Some(next) = current.offset(df, dr) else {
                break;
            };
            // `next` is the last square on this ray if one more step leaves the board.
            // Exclude that rim square from the mask.
            if next.offset(df, dr).is_none() {
                break;
            }
            mask = mask.with(next);
            current = next;
        }
    }
    mask
}

fn carry_rippler_subsets(mask: Bitboard) -> Vec<Bitboard> {
    let mut subsets = Vec::new();
    let mut subset = mask.0;
    loop {
        subsets.push(Bitboard(subset));
        if subset == 0 {
            break;
        }
        subset = (subset - 1) & mask.0;
    }
    subsets
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn sparse_random(&mut self) -> u64 {
        let a = self.next_u64();
        let b = self.next_u64();
        let c = self.next_u64();
        a & b & c
    }
}

fn magic_index(occ: Bitboard, mask: Bitboard, magic: u64, shift: u8) -> usize {
    (((occ & mask).0.wrapping_mul(magic)) >> shift) as usize
}

fn find_magic(
    sq: Square,
    mask: Bitboard,
    reference: impl Fn(Square, Bitboard) -> Bitboard,
    rng_seed: u64,
) -> (u64, u8) {
    let subsets = carry_rippler_subsets(mask);
    let attacks: Vec<Bitboard> = subsets
        .iter()
        .map(|&occ| reference(sq, occ))
        .collect();
    let shift = (64 - mask.count()) as u8;
    let table_size = 1usize << mask.count();
    let mut rng = Rng::new(rng_seed ^ mask.0 ^ sq.index() as u64);

    for _ in 0..1_000_000 {
        let magic = rng.sparse_random();
        let mut used = vec![None::<Bitboard>; table_size];
        let mut ok = true;

        for (&occ, &attack) in subsets.iter().zip(attacks.iter()) {
            let index = magic_index(occ, mask, magic, shift);
            match used[index] {
                None => used[index] = Some(attack),
                Some(existing) if existing == attack => {}
                Some(_) => {
                    ok = false;
                    break;
                }
            }
        }

        if ok {
            return (magic, shift);
        }
    }

    panic!("failed to find magic multiplier for {}", sq);
}

fn init_slider_magics(is_bishop: bool) -> ([MagicEntry; 64], Vec<Bitboard>) {
    let mut magics = std::array::from_fn(|_| MagicEntry {
        mask: Bitboard::EMPTY,
        magic: 0,
        shift: 0,
        offset: 0,
    });
    let mut attack_table = Vec::new();

    for sq in Square::all() {
        let idx = sq.index() as usize;
        let mask = if is_bishop {
            slider_mask(sq, &BISHOP_DIRS)
        } else {
            slider_mask(sq, &ROOK_DIRS)
        };
        let subsets = carry_rippler_subsets(mask);
        let (magic, shift) = if is_bishop {
            find_magic(sq, mask, bishop_attacks_slow, 0xC0FFEE_B105_0001)
        } else {
            find_magic(sq, mask, rook_attacks_slow, 0xC0FFEE_2002_0002)
        };

        let offset = attack_table.len();
        let table_size = 1usize << mask.count();
        attack_table.resize(offset + table_size, Bitboard::EMPTY);

        for occ in subsets {
            let attack = if is_bishop {
                bishop_attacks_slow(sq, occ)
            } else {
                rook_attacks_slow(sq, occ)
            };
            let index = offset + magic_index(occ, mask, magic, shift);
            attack_table[index] = attack;
        }

        magics[idx] = MagicEntry {
            mask,
            magic,
            shift,
            offset,
        };
    }

    (magics, attack_table)
}

fn magic_slider_attacks(entry: &MagicEntry, occ: Bitboard, table: &[Bitboard]) -> Bitboard {
    let index = entry.offset + magic_index(occ, entry.mask, entry.magic, entry.shift);
    table[index]
}

/// Knight attack bitboard from a square (occupancy ignored).
pub fn knight_attacks(sq: Square) -> Bitboard {
    tables().knight_attacks[sq.index() as usize]
}

/// King attack bitboard from a square (occupancy ignored).
pub fn king_attacks(sq: Square) -> Bitboard {
    tables().king_attacks[sq.index() as usize]
}

/// Pawn attack bitboard from a square for the given color (occupancy ignored).
pub fn pawn_attacks(color: Color, sq: Square) -> Bitboard {
    tables().pawn_attacks[color.index()][sq.index() as usize]
}

/// Bishop attack bitboard using magic bitboards and occupancy blockers.
pub fn bishop_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let tables = tables();
    let entry = &tables.bishop_magics[sq.index() as usize];
    magic_slider_attacks(entry, occ, &tables.bishop_attacks)
}

/// Rook attack bitboard using magic bitboards and occupancy blockers.
pub fn rook_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let tables = tables();
    let entry = &tables.rook_magics[sq.index() as usize];
    magic_slider_attacks(entry, occ, &tables.rook_attacks)
}

/// Queen attack bitboard (bishop | rook).
pub fn queen_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    bishop_attacks(sq, occ) | rook_attacks(sq, occ)
}

/// Attack bitboard for non-pawn piece types. Sliders use `occ`; leapers ignore it.
pub fn attacks_bb(piece_type: PieceType, sq: Square, occ: Bitboard) -> Bitboard {
    match piece_type {
        PieceType::Knight => knight_attacks(sq),
        PieceType::Bishop => bishop_attacks(sq, occ),
        PieceType::Rook => rook_attacks(sq, occ),
        PieceType::Queen => queen_attacks(sq, occ),
        PieceType::King => king_attacks(sq),
        PieceType::Pawn => Bitboard::EMPTY,
    }
}

/// Bitboard of pieces of `attacking_color` that attack `sq`.
pub fn attackers_to(
    sq: Square,
    occ: Bitboard,
    knights: Bitboard,
    bishops: Bitboard,
    rooks: Bitboard,
    queens: Bitboard,
    kings: Bitboard,
    pawns: Bitboard,
    attacking_color: Color,
) -> Bitboard {
    let mut attackers = Bitboard::EMPTY;

    attackers |= knight_attacks(sq) & knights;
    attackers |= king_attacks(sq) & kings;
    attackers |= pawn_attacks(!attacking_color, sq) & pawns;

    let bishop_ray = bishop_attacks(sq, occ);
    attackers |= bishop_ray & (bishops | queens);

    let rook_ray = rook_attacks(sq, occ);
    attackers |= rook_ray & (rooks | queens);

    attackers
}

/// Squares strictly between `sq1` and `sq2` on a rank, file, or diagonal; otherwise empty.
pub fn between(sq1: Square, sq2: Square) -> Bitboard {
    line(sq1, sq2) & !Bitboard::from_square(sq1) & !Bitboard::from_square(sq2)
}

/// All squares on the line through `sq1` and `sq2`, including endpoints; otherwise empty.
pub fn line(sq1: Square, sq2: Square) -> Bitboard {
    if sq1 == sq2 {
        return Bitboard::from_square(sq1);
    }

    let df = sq2.file() as i8 - sq1.file() as i8;
    let dr = sq2.rank() as i8 - sq1.rank() as i8;

    let (step_f, step_r) = if df == 0 {
        (0, dr.signum())
    } else if dr == 0 {
        (df.signum(), 0)
    } else if df.abs() == dr.abs() {
        (df.signum(), dr.signum())
    } else {
        return Bitboard::EMPTY;
    };

    let mut bb = Bitboard::EMPTY;
    let mut current = sq1;
    loop {
        bb = bb.with(current);
        if current == sq2 {
            break;
        }
        current = current
            .offset(step_f, step_r)
            .expect("line stays on board between aligned squares");
    }
    bb
}
