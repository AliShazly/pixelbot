use num_derive::FromPrimitive;

use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Write;
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
                write!(f, "Using defaults for missng values:\n|")?;
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
    YDivisor,
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
            YDivisor => Float(Bounded::new(1.3, 1.0..=2.0)),
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
            _ => panic!("Default values not exhaustive"),
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
    (Keycode, i32),
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

#[derive(Debug)]
pub struct Config {
    map: HashMap<CfgKey, ValType>,
    pub is_dirty: bool,
}

impl Config {
    fn new(mut map: HashMap<CfgKey, ValType>) -> Self {
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
        let content: String = CfgKey::iter()
            .map(|k| self.map.get_key_value(&k).unwrap())
            .map(|(k, v)| format!("{} = {}\n", k.as_string(), v))
            .collect();
        let mut outfile = File::create(Path::new(path))?;
        outfile.write_all(content.as_bytes())
    }

    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let key_lookup: HashMap<String, CfgKey> =
            HashMap::from_iter(CfgKey::iter().map(|k| k.as_string()).zip(CfgKey::iter()));
        let mut map: HashMap<CfgKey, ValType> = HashMap::new();
        let infile = File::open(Path::new(path))?;
        for (line_num, line) in BufReader::new(infile).lines().enumerate() {
            let line_num = (line_num as u32) + 1;
            let line_processed: String = line?
                .chars()
                .filter(|x| *x != ' ') // removing whitespace
                .take_while(|x| *x != '#') // ending line at first comment
                .collect();
            if line_processed.is_empty() {
                continue;
            }
            let line_split: Vec<&str> = line_processed.split('=').collect();
            if line_split.len() != 2 {
                return Err(
                    ParseError::Parse(line_num, "Multiple delimiters found".to_string()).into(),
                );
            }

            let key = key_lookup
                .get(
                    &line_split[0]
                        .parse::<String>()
                        .map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?,
                )
                .ok_or(ParseError::InvalidKey(line_num))?;

            // matching the default value for type info
            let value = match key.default_val() {
                ValType::Keycode(_) => ValType::Keycode(
                    line_split[1]
                        .parse::<i32>()
                        .map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?,
                ),
                ValType::Unsigned(v) => {
                    let val = line_split[1]
                        .parse::<u32>()
                        .map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?;
                    if !v.bounds.contains(&val) {
                        return Err(Box::new(ParseError::OutOfBounds(line_num)));
                    }
                    ValType::Unsigned(Bounded::new(val, v.bounds))
                }
                ValType::Float(v) => {
                    let val = line_split[1]
                        .parse::<f32>()
                        .map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?;
                    if !v.bounds.contains(&val) {
                        return Err(Box::new(ParseError::OutOfBounds(line_num)));
                    }
                    ValType::Float(Bounded::new(val, v.bounds))
                }
                ValType::ColorRgb8(_) => {
                    let mut rgb = Vec::with_capacity(3);
                    for res in line_split[1].split(',').map(|num| num.parse::<u8>()) {
                        rgb.push(res.map_err(|e| ParseError::Parse(line_num, format!("{}", e)))?)
                    }
                    if rgb.len() != 3 {
                        return Err(Box::new(ParseError::Parse(
                            line_num,
                            "Color tuple doesn't have 3 elements".to_string(),
                        )));
                    }
                    ValType::ColorRgb8(Color::new(rgb[0], rgb[1], rgb[2], 255))
                }
            };
            map.insert(*key, value);
        }

        let unused_keys: Vec<CfgKey> = CfgKey::iter().filter(|k| !map.contains_key(k)).collect();
        if unused_keys.is_empty() {
            Ok(Config::new(map))
        } else {
            // Config::new() auto fills in unused keys with defaults
            Err(Box::new(ParseError::NotExhaustive(
                Config::new(map),
                unused_keys,
            )))
        }
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
