//! Random UUID identities shared by workspace records and document sessions.
#[must_use]
/// # Panics
/// Panics if the operating system cannot provide random bytes.
pub fn unique_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("operating system randomness");
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}
