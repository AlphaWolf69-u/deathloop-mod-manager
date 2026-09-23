//! Explicit layouts for inspected binaries; never infer offsets from a store name.
#[derive(Clone, Copy)]
pub struct Layout {
    pub name: &'static str,
    pub timestamp: u32,
    pub image_size: u32,
    pub version: usize,
    pub suffix: usize,
    pub empty: usize,
    pub protection: usize,
    pub initializers: [(usize, usize); 3],
    pub profile: usize,
    pub compatibility: usize,
    pub builder: usize,
    pub retail: usize,
    pub transport: usize,
    pub campaign_registry: usize,
    pub map_registry: usize,
    pub world: usize,
}
pub const STEAM: Layout = Layout {
    name: "steam-1.820.5.1",
    timestamp: 0x69B028FB,
    image_size: 0x2130E000,
    version: 0x2FE3EA8,
    suffix: 0x2F47E70,
    empty: 0x25CADB8,
    protection: 0x20ED1B0,
    initializers: [
        (0x23271B0, 0xB662B0),
        (0x23271B8, 0xB662F0),
        (0x2327300, 0xB66880),
    ],
    profile: 0x333A150,
    compatibility: 0x5CE1DF8,
    builder: 0xFB1D80,
    retail: 0x2FE3E70,
    transport: 0x332D3A8,
    campaign_registry: 0x35BDA40,
    map_registry: 0x3340F60,
    world: 0x5BD1010,
};
pub const EPIC: Layout = Layout {
    name: "epic-1.820.5.1",
    timestamp: 0x64BA49F8,
    image_size: 0x21E05000,
    version: 0x2FDEDD8,
    suffix: 0x2F42E50,
    empty: 0x25C7D78,
    protection: 0x20E9960,
    initializers: [
        (0x23241D8, 0xB65ED0),
        (0x23241E0, 0xB65F10),
        (0x2324328, 0xB664A0),
    ],
    profile: 0x3334498,
    compatibility: 0x5CDC058,
    builder: 0xF45550,
    retail: 0x2FDEDA0,
    transport: 0x3327928,
    campaign_registry: 0x35B7D50,
    map_registry: 0x333B260,
    world: 0x5BCB378,
};
pub fn identify(timestamp: u32, image_size: u32) -> Option<Layout> {
    [STEAM, EPIC]
        .into_iter()
        .find(|l| l.timestamp == timestamp && l.image_size == image_size)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_layout_identity() {
        assert_eq!(
            identify(EPIC.timestamp, EPIC.image_size).unwrap().version,
            0x2FDEDD8
        );
        assert_eq!(
            identify(STEAM.timestamp, STEAM.image_size).unwrap().version,
            0x2FE3EA8
        );
        assert!(identify(EPIC.timestamp, STEAM.image_size).is_none());
    }
}
