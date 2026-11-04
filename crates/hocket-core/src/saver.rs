//! Battery saver levels: what each one turns off, and which is in force.
//!
//! The level is a device-local setting (`battery.level`). While the device
//! itself saves power (`Command::SetPowerSave`) and `battery.autoEngage` is
//! on, `battery.autoLevel` takes over, unless the chosen level already goes
//! at least as far: a level the user picked is never weakened. `Custom` is
//! the `battery.custom` feature set.

use crate::api::{BatterySaver, SaverFeature, SaverLevel};
use SaverFeature::*;

const LIGHT: &[SaverFeature] = &[StillBackground, StillSeekWave, LighterLyrics];
const MEDIUM: &[SaverFeature] = &[StillBackground, StillSeekWave, LighterLyrics, RefreshCap, SlowSync];
const MAX: &[SaverFeature] = &[
    StillBackground,
    StillSeekWave,
    LighterLyrics,
    RefreshCap,
    SlowSync,
    SimpleLyrics,
    NoImmersiveArtwork,
    NoArtworkBlur,
    SmallArtwork,
];

/// Every feature, in the order a settings screen lists them.
pub const ALL: &[SaverFeature] = MAX;

/// What a preset level turns off; `Custom` is [`features`]'s `custom`.
pub fn preset(level: SaverLevel) -> &'static [SaverFeature] {
    match level {
        SaverLevel::Off | SaverLevel::Custom => &[],
        SaverLevel::Light => LIGHT,
        SaverLevel::Medium => MEDIUM,
        SaverLevel::Max => MAX,
    }
}

/// What `level` turns off, sorted and without repeats.
pub fn features(level: SaverLevel, custom: &[SaverFeature]) -> Vec<SaverFeature> {
    let mut out: Vec<SaverFeature> = match level {
        SaverLevel::Custom => custom.to_vec(),
        other => preset(other).to_vec(),
    };
    out.sort();
    out.dedup();
    out
}

fn rank(level: SaverLevel) -> Option<u8> {
    match level {
        SaverLevel::Off => Some(0),
        SaverLevel::Light => Some(1),
        SaverLevel::Medium => Some(2),
        SaverLevel::Max => Some(3),
        SaverLevel::Custom => None,
    }
}

/// The battery saver in force.
pub fn resolve(
    chosen: SaverLevel,
    auto_level: SaverLevel,
    auto_engage: bool,
    power_save: bool,
    custom: &[SaverFeature],
) -> BatterySaver {
    let auto = auto_engage
        && power_save
        && auto_level != SaverLevel::Off
        && match (rank(chosen), rank(auto_level)) {
            (Some(0), _) => true,
            (Some(c), Some(a)) => a > c,
            // A custom level, chosen or automatic, is not ranked against a
            // preset: only an "off" choice gives way to it.
            _ => false,
        };
    let level = if auto { auto_level } else { chosen };
    BatterySaver { level, automatic: auto, features: features(level, custom) }
}

/// `battery.custom`'s value: a list of feature names.
pub fn parse_custom(value: &serde_json::Value) -> Vec<SaverFeature> {
    serde_json::from_value::<Vec<SaverFeature>>(value.clone()).unwrap_or_default()
}

/// `battery.level` / `battery.autoLevel`'s value.
pub fn parse_level(value: &serde_json::Value) -> Option<SaverLevel> {
    serde_json::from_value(value.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use SaverLevel::*;

    #[test]
    fn presets_nest() {
        for pair in [Light, Medium, Max].windows(2) {
            assert!(preset(pair[0]).iter().all(|f| preset(pair[1]).contains(f)));
            assert!(preset(pair[1]).len() > preset(pair[0]).len());
        }
        assert!(preset(Off).is_empty());
    }

    #[test]
    fn chosen_level_holds_without_power_save() {
        let s = resolve(Light, Max, true, false, &[]);
        assert_eq!((s.level, s.automatic), (Light, false));
        assert_eq!(s.features, features(Light, &[]));
    }

    #[test]
    fn power_save_raises_but_never_lowers() {
        assert_eq!(resolve(Off, Medium, true, true, &[]).level, Medium);
        assert!(resolve(Off, Medium, true, true, &[]).automatic);
        assert_eq!(resolve(Light, Medium, true, true, &[]).level, Medium);
        let max = resolve(Max, Medium, true, true, &[]);
        assert_eq!((max.level, max.automatic), (Max, false));
        assert_eq!(resolve(Off, Medium, false, true, &[]).level, Off);
    }

    #[test]
    fn custom_only_replaces_off() {
        let custom = [SimpleLyrics, StillBackground];
        let auto = resolve(Off, Custom, true, true, &custom);
        assert_eq!((auto.level, auto.features.clone()), (Custom, vec![StillBackground, SimpleLyrics]));
        assert_eq!(resolve(Light, Custom, true, true, &custom).level, Light);
        assert_eq!(resolve(Custom, Max, true, true, &custom).level, Custom);
    }

    #[test]
    fn custom_value_parses_names() {
        let v = serde_json::json!(["simpleLyrics", "refreshCap", "simpleLyrics"]);
        assert_eq!(features(Custom, &parse_custom(&v)), vec![RefreshCap, SimpleLyrics]);
        assert!(parse_custom(&serde_json::json!("nope")).is_empty());
    }
}
