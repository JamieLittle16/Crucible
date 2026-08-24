//! Java-compatible offline-player UUID derivation for the admitted 26.2 Login path.
//!
//! Minecraft's offline Login branch calls `UUID.nameUUIDFromBytes` over the UTF-8 bytes of
//! `"OfflinePlayer:" + player_name`. `OpenJDK` 25 defines that operation as MD5 followed by UUID
//! version-3 / IETF-variant bit normalization. This module implements only that compatibility law;
//! it is not a cryptographic API.

const OFFLINE_PREFIX: &[u8; 14] = b"OfflinePlayer:";
const MAX_PLAYER_NAME_ASCII_BYTES: usize = 16;
const MAX_OFFLINE_INPUT_BYTES: usize = OFFLINE_PREFIX.len() + MAX_PLAYER_NAME_ASCII_BYTES;
const MD5_BLOCK_BYTES: usize = 64;

const SHIFT: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

const TABLE: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// Returns the exact 16 UUID bytes produced by Java's offline-profile identity law.
pub(crate) fn offline_player_uuid(player_name: &str) -> [u8; 16] {
    let name = player_name.as_bytes();
    debug_assert!(name.len() <= MAX_PLAYER_NAME_ASCII_BYTES);
    debug_assert!(name.iter().all(|byte| (b'!'..=b'~').contains(byte)));

    let mut input = [0_u8; MAX_OFFLINE_INPUT_BYTES];
    input[..OFFLINE_PREFIX.len()].copy_from_slice(OFFLINE_PREFIX);
    input[OFFLINE_PREFIX.len()..OFFLINE_PREFIX.len() + name.len()].copy_from_slice(name);

    let mut uuid = md5_single_block(&input[..OFFLINE_PREFIX.len() + name.len()]);
    uuid[6] = (uuid[6] & 0x0f) | 0x30;
    uuid[8] = (uuid[8] & 0x3f) | 0x80;
    uuid
}

fn md5_single_block(input: &[u8]) -> [u8; 16] {
    // The admitted input is at most 30 bytes, so MD5 padding always fits one 64-byte block.
    assert!(input.len() + 9 <= MD5_BLOCK_BYTES);

    let mut block = [0_u8; MD5_BLOCK_BYTES];
    block[..input.len()].copy_from_slice(input);
    block[input.len()] = 0x80;
    let bit_len = u64::try_from(input.len())
        .expect("offline UUID input length fits u64")
        .wrapping_mul(8);
    block[MD5_BLOCK_BYTES - 8..].copy_from_slice(&bit_len.to_le_bytes());

    let mut words = [0_u32; 16];
    for (index, word) in words.iter_mut().enumerate() {
        let start = index * 4;
        *word = u32::from_le_bytes([
            block[start],
            block[start + 1],
            block[start + 2],
            block[start + 3],
        ]);
    }

    let mut a = 0x6745_2301_u32;
    let mut b = 0xefcd_ab89_u32;
    let mut c = 0x98ba_dcfe_u32;
    let mut d = 0x1032_5476_u32;
    let initial = [a, b, c, d];

    for index in 0..64 {
        let (function, word_index) = match index {
            0..=15 => ((b & c) | ((!b) & d), index),
            16..=31 => ((d & b) | ((!d) & c), (5 * index + 1) % 16),
            32..=47 => (b ^ c ^ d, (3 * index + 5) % 16),
            _ => (c ^ (b | !d), (7 * index) % 16),
        };

        let next_d = c;
        let next_c = b;
        let mixed = a
            .wrapping_add(function)
            .wrapping_add(TABLE[index])
            .wrapping_add(words[word_index]);
        let next_b = b.wrapping_add(mixed.rotate_left(SHIFT[index]));
        a = d;
        b = next_b;
        c = next_c;
        d = next_d;
    }

    let state = [
        initial[0].wrapping_add(a),
        initial[1].wrapping_add(b),
        initial[2].wrapping_add(c),
        initial[3].wrapping_add(d),
    ];
    let mut digest = [0_u8; 16];
    for (index, value) in state.into_iter().enumerate() {
        let start = index * 4;
        digest[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::{md5_single_block, offline_player_uuid};

    fn parse_uuid(text: &str) -> [u8; 16] {
        let compact: String = text.chars().filter(|character| *character != '-').collect();
        let bytes = hex(&compact);
        bytes.try_into().expect("UUID is 16 bytes")
    }

    fn hex(text: &str) -> Vec<u8> {
        assert!(text.len().is_multiple_of(2));
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let high = digit(pair[0]);
                let low = digit(pair[1]);
                (high << 4) | low
            })
            .collect()
    }

    const fn digit(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            _ => panic!("test hex digit"),
        }
    }

    #[test]
    fn md5_standard_vectors_cover_the_admitted_single_block_domain() {
        for (input, expected) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234",
                "e8be43556a680604ebb5369c8540e180",
            ),
        ] {
            assert_eq!(md5_single_block(input.as_bytes()).as_slice(), hex(expected));
        }
    }

    #[test]
    fn openjdk_oracle_vectors_are_byte_exact() {
        for (name, expected) in [
            ("CrucibleR1", "18030a72-9cb6-38f0-a5ba-e1c75f1314e9"),
            ("Player", "a01e3843-e521-3998-958a-f459800e4d11"),
            ("A_B9", "3ed12607-2255-3097-a2e0-8df867708688"),
            ("aaaaaaaaaaaaaaaa", "7259babb-8b73-37de-88fc-86fa22252071"),
            ("Z9_0", "d5cb2d97-1164-3e9a-b4f7-0d3cb3067807"),
            ("Stato16", "682014fe-ad63-3699-aada-79aa08d95b45"),
        ] {
            assert_eq!(
                offline_player_uuid(name),
                parse_uuid(expected),
                "name={name}"
            );
        }
    }
}
