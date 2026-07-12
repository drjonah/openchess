//! Quantized NNUE network: FT columns + dense head + embed/load (P6-06).
//!
//! Binary format (`OCNNv002`):
//! ```text
//! magic[8] = b"OCNNv002"
//! l1,l2,l3 : u32 LE
//! ft_w: i16[FEATURE_COUNT * l1] LE
//! ft_bias: i16[l1] LE
//! fc1_w: i8[l2 * 2*l1]
//! fc1_b: i32[l2]
//! fc2_w: i8[l3 * l2]
//! fc2_b: i32[l3]
//! out_w: i8[l3]
//! out_b: i32
//! scale: i32   // divide after out for centipawns
//! ```

use super::features::{feature_slot, feature_sq, FEATURE_COUNT, L1_SIZE};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Hidden layer widths for the embedded bootstrap net.
pub const L2_SIZE: usize = 32;
pub const L3_SIZE: usize = 32;

const MAGIC: &[u8; 8] = b"OCNNv002";

static EMBEDDED: OnceLock<Arc<Network>> = OnceLock::new();

/// Quantized HalfKA FT + dense MLP over dual ClippedReLU accumulators.
#[derive(Clone, Debug)]
pub struct Network {
    pub l1: usize,
    pub l2: usize,
    pub l3: usize,
    /// Dense FT columns: `ft_w[feature * l1 + dim]`.
    pub ft_w: Vec<i16>,
    pub ft_bias: Vec<i16>,
    pub fc1_w: Vec<i8>,
    pub fc1_b: Vec<i32>,
    pub fc2_w: Vec<i8>,
    pub fc2_b: Vec<i32>,
    pub out_w: Vec<i8>,
    pub out_b: i32,
    /// Divide raw output by this to get centipawns.
    pub scale: i32,
}

impl Network {
    /// Shared embedded bootstrap net (built once).
    pub fn embedded_shared() -> Arc<Network> {
        EMBEDDED
            .get_or_init(|| Arc::new(Self::build_bootstrap()))
            .clone()
    }

    /// Compile-time-shaped embedded bootstrap net (material-aware FT).
    pub fn embedded() -> Self {
        Self::build_bootstrap()
    }

    /// Material-distilled HalfKA bootstrap used until Bullet-trained nets ship.
    ///
    /// Dim 0 carries piece values (ours positive); the dense head reads
    /// `crelu(stm[0]) - crelu(nstm[0])` so startpos ≈ 0 and hanging queens swing.
    fn build_bootstrap() -> Self {
        let l1 = L1_SIZE;
        let l2 = L2_SIZE;
        let l3 = L3_SIZE;
        let in_dim = 2 * l1;
        let mut net = Self {
            l1,
            l2,
            l3,
            ft_w: vec![0; FEATURE_COUNT * l1],
            ft_bias: vec![0; l1],
            fc1_w: vec![0; l2 * in_dim],
            fc1_b: vec![0; l2],
            fc2_w: vec![0; l3 * l2],
            fc2_b: vec![0; l3],
            out_w: vec![0; l3],
            out_b: 0,
            scale: 1,
        };

        // Mild positive bias so ClippedReLU sits mid-range with material on dim 0.
        net.ft_bias[0] = 16;
        for dim in 1..l1 {
            net.ft_bias[dim] = 4;
        }

        for feat in 0..FEATURE_COUNT {
            let slot = feature_slot(feat);
            let sq = feature_sq(feat);
            let ours = slot < 6;
            let pt = slot % 6;
            let material = match pt {
                0 => 100i16, // pawn
                1 => 320,
                2 => 330,
                3 => 500,
                4 => 900,
                _ => 0, // king (enemy only)
            };
            let base = feat * l1;
            // Dim 0: material for our pieces only (perspective-relative).
            // Scale enough to stay inside ClippedReLU [0,127] with a full set.
            if ours && material != 0 {
                net.ft_w[base] = material / 64;
            }
            // Extra dims: cheap PST-ish + deterministic capacity for later training.
            if ours {
                let rank = (sq / 8) as i16;
                let file = (sq % 8) as i16;
                let center = 3 - (file - 3).abs() - (rank - 3).abs() / 2;
                if l1 > 1 {
                    net.ft_w[base + 1] = center;
                }
                if l1 > 2 && pt == 0 {
                    // Encourage advanced pawns slightly.
                    net.ft_w[base + 2] = rank / 2;
                }
            } else if pt == 5 && l1 > 3 {
                // Enemy king presence — small positional dim.
                net.ft_w[base + 3] = 2;
            }
            // Fill remaining dims with tiny deterministic noise so FT is full-rank.
            for dim in 4..l1.min(16) {
                net.ft_w[base + dim] = stub_i16(feat.wrapping_mul(dim + 1)) / 64;
            }
        }

        // Dense head: neuron 0 ≈ stm[0] - nstm[0]; pass through to output.
        // fc1: h1[0] = bias + stm[0]*1 + nstm[0]*(-1)
        net.fc1_w[0 * in_dim + 0] = 1; // stm dim0
        net.fc1_w[0 * in_dim + l1] = -1; // nstm dim0
        // Positional blend into neuron 1.
        if in_dim > 1 {
            net.fc1_w[1 * in_dim + 1] = 1;
            net.fc1_w[1 * in_dim + l1 + 1] = -1;
        }
        net.fc1_b[0] = 64; // keep crelu active for near-equal material
        net.fc1_b[1] = 64;

        // fc2: identity-ish on first two neurons.
        net.fc2_w[0 * l2 + 0] = 1;
        net.fc2_w[1 * l2 + 1] = 1;
        net.fc2_b[0] = 0;
        net.fc2_b[1] = 0;

        // out ≈ recover centipawns: each FT material unit is piece/64, so ×64.
        net.out_w[0] = 64;
        net.out_w[1] = 4;
        net.out_b = -(64 * 64 + 4 * 64);
        net.scale = 1;
        net
    }

    /// FT column slice for `feature` (length `l1`).
    #[inline]
    pub fn ft_column(&self, feature: usize) -> &[i16] {
        let base = feature * self.l1;
        &self.ft_w[base..base + self.l1]
    }

    pub fn load_file(path: &Path) -> io::Result<Self> {
        let data = fs::read(path)?;
        Self::from_bytes(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let mut cur = io::Cursor::new(data);
        let mut magic = [0u8; 8];
        cur.read_exact(&mut magic)
            .map_err(|e| format!("short header: {e}"))?;
        if &magic != MAGIC {
            return Err(format!("bad magic: {:?} (expected OCNNv002)", magic));
        }
        let l1 = read_u32(&mut cur)? as usize;
        let l2 = read_u32(&mut cur)? as usize;
        let l3 = read_u32(&mut cur)? as usize;
        if l1 == 0 || l2 == 0 || l3 == 0 || l1 > 4096 || l2 > 1024 || l3 > 1024 {
            return Err(format!("implausible layer sizes {l1}/{l2}/{l3}"));
        }
        if l1 != L1_SIZE {
            return Err(format!(
                "network L1={l1} does not match engine L1_SIZE={L1_SIZE}"
            ));
        }
        let in_dim = 2 * l1;
        let mut net = Self {
            l1,
            l2,
            l3,
            ft_w: vec![0; FEATURE_COUNT * l1],
            ft_bias: vec![0; l1],
            fc1_w: vec![0; l2 * in_dim],
            fc1_b: vec![0; l2],
            fc2_w: vec![0; l3 * l2],
            fc2_b: vec![0; l3],
            out_w: vec![0; l3],
            out_b: 0,
            scale: 16,
        };
        read_i16_slice(&mut cur, &mut net.ft_w)?;
        read_i16_slice(&mut cur, &mut net.ft_bias)?;
        read_i8_slice(&mut cur, &mut net.fc1_w)?;
        read_i32_slice(&mut cur, &mut net.fc1_b)?;
        read_i8_slice(&mut cur, &mut net.fc2_w)?;
        read_i32_slice(&mut cur, &mut net.fc2_b)?;
        read_i8_slice(&mut cur, &mut net.out_w)?;
        net.out_b = read_i32(&mut cur)?;
        net.scale = read_i32(&mut cur)?;
        if net.scale == 0 {
            return Err("scale must be non-zero".into());
        }
        Ok(net)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        write_u32(&mut out, self.l1 as u32);
        write_u32(&mut out, self.l2 as u32);
        write_u32(&mut out, self.l3 as u32);
        for w in &self.ft_w {
            write_i16(&mut out, *w);
        }
        for b in &self.ft_bias {
            write_i16(&mut out, *b);
        }
        out.extend(self.fc1_w.iter().map(|w| *w as u8));
        for b in &self.fc1_b {
            write_i32(&mut out, *b);
        }
        out.extend(self.fc2_w.iter().map(|w| *w as u8));
        for b in &self.fc2_b {
            write_i32(&mut out, *b);
        }
        out.extend(self.out_w.iter().map(|w| *w as u8));
        write_i32(&mut out, self.out_b);
        write_i32(&mut out, self.scale);
        out
    }

    pub fn write_file(&self, path: &Path) -> io::Result<()> {
        fs::write(path, self.to_bytes())
    }
}

#[inline]
fn stub_i16(x: usize) -> i16 {
    let mut v = x.wrapping_mul(0x9E37_79B9);
    v ^= v >> 16;
    ((v % 17) as i16).wrapping_sub(8)
}

fn read_u32(r: &mut impl Read) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(buf))
}

fn read_i32(r: &mut impl Read) -> Result<i32, String> {
    Ok(read_u32(r)? as i32)
}

fn read_i16(r: &mut impl Read) -> Result<i16, String> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(i16::from_le_bytes(buf))
}

fn read_i8_slice(r: &mut impl Read, dst: &mut [i8]) -> Result<(), String> {
    let mut buf = vec![0u8; dst.len()];
    r.read_exact(&mut buf).map_err(|e| e.to_string())?;
    for (i, b) in buf.into_iter().enumerate() {
        dst[i] = b as i8;
    }
    Ok(())
}

fn read_i16_slice(r: &mut impl Read, dst: &mut [i16]) -> Result<(), String> {
    for slot in dst.iter_mut() {
        *slot = read_i16(r)?;
    }
    Ok(())
}

fn read_i32_slice(r: &mut impl Read, dst: &mut [i32]) -> Result<(), String> {
    for slot in dst.iter_mut() {
        *slot = read_i32(r)?;
    }
    Ok(())
}

fn write_u32(w: &mut impl Write, v: u32) {
    let _ = w.write_all(&v.to_le_bytes());
}

fn write_i32(w: &mut impl Write, v: i32) {
    write_u32(w, v as u32);
}

fn write_i16(w: &mut impl Write, v: i16) {
    let _ = w.write_all(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn embedded_roundtrips_bytes() {
        let net = Network::embedded_shared();
        let bytes = net.to_bytes();
        let loaded = Network::from_bytes(&bytes).unwrap();
        assert_eq!(loaded.l1, net.l1);
        assert_eq!(loaded.l2, net.l2);
        assert_eq!(loaded.l3, net.l3);
        assert_eq!(loaded.ft_w.len(), net.ft_w.len());
        assert_eq!(loaded.ft_bias, net.ft_bias);
        assert_eq!(loaded.fc1_w, net.fc1_w);
        assert_eq!(loaded.out_b, net.out_b);
        assert_eq!(loaded.scale, net.scale);
    }

    #[test]
    fn rejects_bad_magic() {
        let err = Network::from_bytes(b"BADMAGIC0").unwrap_err();
        assert!(err.contains("bad magic"));
    }

    #[test]
    fn bootstrap_startpos_near_zero() {
        crate::lookup::initialize();
        let board = crate::board::Board::startpos();
        let net = Network::embedded_shared();
        let score = crate::eval::nnue::evaluate(&board, &net);
        assert!(
            score.abs() < 150,
            "bootstrap startpos should be near 0, got {score}"
        );
    }

    #[test]
    fn bootstrap_missing_queen_is_negative() {
        crate::lookup::initialize();
        let mut board = crate::board::Board::startpos();
        board.remove_piece(crate::types::Square::from_str("d1").unwrap());
        board.rehash();
        let net = Network::embedded_shared();
        let score = crate::eval::nnue::evaluate(&board, &net);
        assert!(
            score < -400,
            "missing white queen should be strongly negative, got {score}"
        );
    }
}
