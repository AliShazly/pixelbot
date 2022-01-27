use num_derive::FromPrimitive;

use rustc_hash::{FxHashMap, FxHashSet};
use std::error::Error;
use std::fmt;
use std::fmt::Display;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Write;
use std::lazy::SyncLazy;
use std::ops::RangeInclusive;
use std::path::Path;

use crate::image::Color;

#[derive(Debug)]
pub enum ParseError {
    Parse(u32, String),
    InvalidKey(u32),
    OutOfBounds(u32),
    NotExhaustive(Config, Vec<CfgKey>),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Parse(line_num, ref err_string) => {
                write!(f, "Parse error on line {} => {}", line_num, err_string)
            }
            Self::InvalidKey(line_num) => write!(f, "Invalid key on line {}", line_num),
            Self::OutOfBounds(line_num) => write!(f, "Out of bounds value on line {}", line_num),
            Self::NotExhaustive(_, ref missing_keys) => {
                write!(f, "Using defaults for missing values:\n|")?;
                for res in missing_keys
                    .iter()
                    .map(|key| write!(f, " {} |", key.as_string()))
                {
                    res?
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Hash, PartialEq, Eq, FromPrimitive, Clone, Copy)]
pub enum CfgKey {
    CropW = 0,
    CropH,
    ColorThresh,
    AimDivisor,
    YMultiplier,
    Fps,
    MaxAutoclickSleepMs,
    MinAutoclickSleepMs,
    AimDurationMicros,
    AimSteps,
    AimKeycode,
    AutoclickKeycode,
    ToggleAimKeycode,
    ToggleAutoclickKeycode,
    FakeLmbKeycode,
    TargetColor,
    _Size, // Last item get assigned the size of the enum
}
const N_CFG_KEYS: usize = CfgKey::_Size as usize;

impl CfgKey {
    pub fn default_val(&self) -> ValType {
        use CfgKey::*;
        use ValType::*;

        match *self {
            CropW => Unsigned(Bounded::new(1152, 0..=2560 - 1)), // Bounds set at runtime for crop_w & crop_h
            CropH => Unsigned(Bounded::new(592, 0..=1440 - 1)),
            ColorThresh => Float(Bounded::new(0.83, 0.001..=0.999)),
            AimDivisor => Float(Bounded::new(3., 1.0..=10.0)),
            YMultiplier => Float(Bounded::new(0.9, 0.0..=1.0)),
            Fps => Unsigned(Bounded::new(144, 1..=240)),
            MaxAutoclickSleepMs => Unsigned(Bounded::new(90, 0..=100)),
            MinAutoclickSleepMs => Unsigned(Bounded::new(50, 0..=100)),
            AimDurationMicros => Unsigned(Bounded::new(50, 0..=2000)),
            AimSteps => Unsigned(Bounded::new(2, 1..=10)),
            AimKeycode => Keycode(1),
            AutoclickKeycode => Keycode(1),
            ToggleAimKeycode => Keycode(190),
            ToggleAutoclickKeycode => Keycode(188),
            FakeLmbKeycode => Keycode(4),
            TargetColor => ColorRgb8(Color::<u8>::new(196, 58, 172, 255)),
            _Size => panic!(),
        }
    }

    // Uses FromPrimitive to convert integer into variant of cfgkey struct
    pub fn iter() -> impl Iterator<Item = Self> {
        (0..N_CFG_KEYS).map(|i| num::FromPrimitive::from_usize(i).unwrap())
    }

    pub fn is_keycode(&self) -> bool {
        matches!(self.default_val(), ValType::Keycode(_))
    }

    pub fn as_string(&self) -> String {
        camel_to_snake(&format!("{:?}", self))
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Bounded<T> {
    pub val: T,
    pub bounds: RangeInclusive<T>,
}

impl<T> Bounded<T> {
    pub fn new(val: T, bounds: RangeInclusive<T>) -> Self {
        Self { val, bounds }
    }
}

macro_rules! enum_valtype {
    ($(($name: ident, $val_typ: ty)),*) => {
        #[derive(Debug, PartialEq, Clone)]
        pub enum ValType {
            $(
                $name($val_typ),
            )*
        }

        $(
            impl From<ValType> for $val_typ {
                fn from(v: ValType) -> $val_typ {
                    match v {
                        ValType::$name(v) => v,
                        _ => panic!("ValType from/into wrong type, i tried so hard to make this a compile time error ;_;")
                    }
                }
            }
        )*
    };
}
enum_valtype!(
    (Keycode, u16),
    (Unsigned, Bounded<u32>),
    (Float, Bounded<f32>),
    (ColorRgb8, Color<u8>)
);

impl Display for ValType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keycode(v) => write!(f, "{}", v),
            Self::Unsigned(v) => write!(f, "{}", v.val),
            Self::Float(v) => write!(f, "{}", v.val),
            Self::ColorRgb8(c) => write!(f, "{}, {}, {}", c.r, c.g, c.b),
        }
    }
}

struct LineData {
    key_val_pair: Option<(CfgKey, ValType)>,
    comment: Option<String>,
}

#[derive(Debug)]
pub struct Config {
    map: FxHashMap<CfgKey, ValType>,
    pub is_dirty: bool,
}

impl Config {
    fn new(mut map: FxHashMap<CfgKey, ValType>) -> Self {
        CfgKey::iter().for_each(|key| {
            map.entry(key).or_insert_with(|| key.default_val());
        });
        Self { map, is_dirty: false }
    }

    pub fn default() -> Self {
        Self::new(CfgKey::iter().map(|key| (key, key.default_val())).collect())
    }

    pub fn get(&self, key: CfgKey) -> ValType {
        self.map.get(&key).unwrap().clone()
    }

    pub fn set_val(&mut self, key: CfgKey, new_val: ValType) -> Result<(), &'static str> {
        const ERR_MSG: &str = "Value not in bounds";
        match self.map.get_mut(&key).unwrap() {
            ValType::Unsigned(bv) => {
                let new_val: Bounded<_> = new_val.into();
                if bv.bounds.contains(&new_val.val) {
                    bv.val = new_val.val;
                } else {
                    return Err(ERR_MSG);
                }
            }
            ValType::Float(bv) => {
                let new_val: Bounded<_> = new_val.into();
                if bv.bounds.contains(&new_val.val) {
                    bv.val = new_val.val;
                } else {
                    return Err(ERR_MSG);
                }
            }
            ValType::Keycode(kc) => *kc = new_val.into(),
            ValType::ColorRgb8(c) => *c = new_val.into(),
        }
        self.is_dirty = true;
        Ok(())
    }

    pub fn set_bounds(&mut self, key: CfgKey, new_val: ValType) -> Result<(), &'static str> {
        match self.map.get_mut(&key).unwrap() {
            ValType::Unsigned(ref mut val_ref) => {
                let new_val_cast: Bounded<_> = new_val.into();
                val_ref.bounds = new_val_cast.bounds;
                self.is_dirty = true;
                Ok(())
            }
            ValType::Float(ref mut val_ref) => {
                let new_val_cast: Bounded<_> = new_val.into();
                val_ref.bounds = new_val_cast.bounds;
                self.is_dirty = true;
                Ok(())
            }
            _ => Err("No bounds to set"),
        }
    }

    pub fn write_to_file(&self, path: &str) -> std::io::Result<()> {
        let file_path = Path::new(path);
        let mut out_content = "".to_string();
        let mut written_keys = FxHashSet::<CfgKey>::default();

        // overwriting keys already written to file to preserve comments & line ordering
        if let Ok(read_handle) = File::open(file_path) {
            for (line_num, line) in BufReader::new(read_handle).lines().enumerate() {
                let line_num = (line_num as u32) + 1;
                match Self::parse_line(line?, line_num) {
                    Ok(line_data) => {
                        if let Some((k, _)) = line_data.key_val_pair {
                            let val = self.map.get(&k).unwrap();
                            out_content.push_str(&format!("{} = {}", k.as_string(), val));
                            written_keys.insert(k);
                            if line_data.comment.is_some() {
                                out_content.push(' '); // Adding a space before inline comments
                            }
                        }
                        if let Some(comment) = line_data.comment {
                            out_content.push('#');
                            out_content.push_str(&comment);
                        }
                        out_content.push('\n');
                    }
                    Err(_) => continue, // skip lines that don't parse correctly
                };
            }
        }

        // writing the rest of the unwritten values
        out_content.push_str(
            &CfgKey::iter()
                .filter(|k| !written_keys.contains(k))
                .map(|k| self.map.get_key_value(&k).unwrap())
                .map(|(k, v)| format!("{} = {}\n", k.as_string(), v))
                .collect::<String>(),
        );

        File::create(file_path)?.write_all(out_content.as_bytes())
    }

    pub fn from_file(path: &str) -> Result<Self, Box<dyn Error>> {
        let mut out_map: FxHashMap<CfgKey, ValType> = FxHashMap::default();
        let infile = File::open(Path::new(path))?;
        for (line_num, line) in BufReader::new(infile).lines().enumerate() {
            let line_num = (line_num as u32) + 1;
            let LineData { key_val_pair, comment: _ } = Self::parse_line(line?, line_num)?;
            if let Some((k, v)) = key_val_pair {
                out_map.insert(k, v);
            }
        }

        let unused_keys: Vec<CfgKey> = CfgKey::iter()
            .filter(|k| !out_map.contains_key(k))
            .collect();
        if unused_keys.is_empty() {
            Ok(Config::new(out_map))
        } else {
            // Config::new() auto fills in unused keys with defaults
            Err(ParseError::NotExhaustive(Config::new(out_map), unused_keys).into())
        }
    }

    fn parse_line(line: String, line_num: u32) -> Result<LineData, ParseError> {
        static KEY_LOOKUP: SyncLazy<FxHashMap<String, CfgKey>> = SyncLazy::new(|| {
            FxHashMap::from_iter(CfgKey::iter().map(|k| k.as_string()).zip(CfgKey::iter()))
        });

        let (mut key_val, comment) = match line.split_once('#') {
            Some((key_val, comment)) => (key_val.to_string(), Some(comment.to_string())),
            None => (line, None),
        };
        key_val.retain(|c| c != ' ');
        let (key_str, val_str) = match key_val.split_once('=') {
            Some((key_str, val_str)) => (key_str, val_str),
            None => {
                if key_val.is_empty() {
                    return Ok(LineData { key_val_pair: None, comment }); // empty line is valid
                } else {
                    return Err(ParseError::Parse(line_num, "No delimiter".into()));
                }
            }
        };

        let key = KEY_LOOKUP
            .get(key_str)
            .ok_or(ParseError::InvalidKey(line_num))?;

        // matching the default value for type info
        let val = match key.default_val() {
            ValType::Keycode(_) => ValType::Keycode(
                val_str
                    .parse::<u16>()
                    .map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?,
            ),
            ValType::Unsigned(v) => {
                let val = val_str
                    .parse::<u32>()
                    .map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?;
                if !v.bounds.contains(&val) {
                    return Err(ParseError::OutOfBounds(line_num));
                }
                ValType::Unsigned(Bounded::new(val, v.bounds))
            }
            ValType::Float(v) => {
                let val = val_str
                    .parse::<f32>()
                    .map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?;
                if !v.bounds.contains(&val) {
                    return Err(ParseError::OutOfBounds(line_num));
                }
                ValType::Float(Bounded::new(val, v.bounds))
            }
            ValType::ColorRgb8(_) => {
                let mut rgb = [0u8; 3];
                let mut elems = 0;
                for res in val_str.split(',').map(|num| num.parse::<u8>()) {
                    rgb[elems] = res.map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?;
                    elems += 1;
                }
                if elems != 3 {
                    return Err(ParseError::Parse(line_num, "Invalid color".into()));
                }
                ValType::ColorRgb8(Color::new(rgb[0], rgb[1], rgb[2], 255))
            }
        };
        Ok(LineData {
            key_val_pair: Some((*key, val)),
            comment,
        })
    }
}

fn camel_to_snake(camel_str: &str) -> String {
    let mut snake_str = camel_str.to_string().to_lowercase();
    let mut insert_offset = 0;
    camel_str
        .chars()
        .enumerate()
        .skip(1) // Skip the first uppercase letter
        .filter(|(_, char)| char.is_uppercase())
        .for_each(|(idx, _)| {
            snake_str.insert(idx + insert_offset, '_');
            insert_offset += 1;
        });
    snake_str
}
