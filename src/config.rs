use num_enum::TryFromPrimitive;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;
use std::fmt::Display;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Write;
use std::ops::RangeInclusive;

const CFG_FILE_PATH: &str = "./config.cfg";

#[derive(Debug)]
pub enum ReadError {
    Parse(u32),
    InvalidKey(u32),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Parse(line_num) => write!(f, "Parse error on line {}", line_num),
            Self::InvalidKey(line_num) => write!(f, "Invalid key on line {}", line_num),
        }
    }
}

impl std::error::Error for ReadError {}

#[derive(Debug, Hash, PartialEq, Eq, TryFromPrimitive)]
#[repr(usize)]
pub enum CfgKey {
    CropW = 0,
    CropH,
    ColorThresh,
    AimDivisor,
    YDivisor,
    Fps,
    MaxAutoclickSleepMs,
    AimDurationMicros,
    AimSteps,
    AimKeycode,
    ToggleAimKeycode,
    ToggleAutoclickKeycode,
    _Size,
}
const N_CONFIG_ITEMS: usize = CfgKey::_Size as usize;

impl CfgKey {
    // Uses TryFromPrimitive to convert integer into variant of cfgkey struct
    fn iter() -> impl Iterator<Item = CfgKey> {
        (0..N_CONFIG_ITEMS).map(|i| CfgKey::try_from(i).unwrap())
    }

    fn default_val(&self) -> CfgVal {
        match *self {
            CfgKey::CropW => CfgVal::U32(1152, RangeInclusive::new(0, 1920)),
            CfgKey::CropH => CfgVal::U32(592, RangeInclusive::new(0, 1080)),
            CfgKey::ColorThresh => CfgVal::F32(0.83, RangeInclusive::new(0., 1.)),
            CfgKey::AimDivisor => CfgVal::F32(3., RangeInclusive::new(1., 10.)),
            CfgKey::YDivisor => CfgVal::F32(1.3, RangeInclusive::new(1., 2.)),
            CfgKey::Fps => CfgVal::U32(144, RangeInclusive::new(30, 240)),
            CfgKey::MaxAutoclickSleepMs => CfgVal::U32(66, RangeInclusive::new(40, 100)),
            CfgKey::AimDurationMicros => CfgVal::U32(50, RangeInclusive::new(0, 2000)),
            CfgKey::AimSteps => CfgVal::U32(2, RangeInclusive::new(1, 10)),
            CfgKey::AimKeycode => CfgVal::Keycode(1),
            CfgKey::ToggleAimKeycode => CfgVal::Keycode(190),
            CfgKey::ToggleAutoclickKeycode => CfgVal::Keycode(188),
            _ => panic!("Default values not exhaustive"),
        }
    }

    fn as_string(&self) -> String {
        camel_to_snake(format!("{:?}", self).as_str())
    }
}

#[derive(Debug, PartialEq)]
pub enum CfgVal {
    Keycode(i32),
    U32(u32, RangeInclusive<u32>),
    F32(f32, RangeInclusive<f32>),
}

impl Display for CfgVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keycode(v) => write!(f, "{}", v),
            Self::U32(v, _) => write!(f, "{}", v),
            Self::F32(v, _) => write!(f, "{}", v),
        }
    }
}

#[derive(Debug)]
pub struct Config {
    map: HashMap<CfgKey, CfgVal>,
}

impl Config {
    fn new(values: Vec<CfgVal>) -> Self {
        Self {
            map: HashMap::from_iter(CfgKey::iter().zip(values)),
        }
    }

    pub fn default() -> Self {
        Self::new(
            CfgKey::iter()
                .map(|key| key.default_val())
                .collect::<Vec<CfgVal>>(),
        )
    }

    pub fn write_to_file(&self) -> std::io::Result<()> {
        let content: String = CfgKey::iter()
            .map(|k| self.map.get_key_value(&k).unwrap())
            .map(|(k, v)| format!("{} = {}\n", k.as_string(), v))
            .collect();
        let mut outfile = File::create(CFG_FILE_PATH)?;
        outfile.write_all(content.as_bytes())
    }

    pub fn from_file() -> Result<Self, Box<dyn std::error::Error>> {
        let key_lookup: HashMap<String, CfgKey> =
            HashMap::from_iter(CfgKey::iter().map(|k| k.as_string()).zip(CfgKey::iter()));
        let infile = File::open(CFG_FILE_PATH)?;
        let mut values: Vec<CfgVal> = Vec::new();
        for (line_num, line) in BufReader::new(infile).lines().enumerate() {
            let line_num = (line_num as u32) + 1;
            let line_processed: String = line?
                .chars()
                .filter(|x| *x != ' ') // removing whitespace
                .take_while(|x| *x != '#') // ending line at first comment
                .collect();
            let line_split: Vec<&str> = line_processed.split('=').collect();
            if line_split.len() != 2 {
                return Err(ReadError::Parse(line_num).into());
            }

            let key = key_lookup
                .get(
                    &line_split[0]
                        .parse::<String>()
                        .map_err(|_| ReadError::Parse(line_num))?,
                )
                .ok_or(ReadError::InvalidKey(line_num))?;

            // matching the default value for type info
            let value = match key.default_val() {
                CfgVal::Keycode(_) => CfgVal::Keycode(
                    line_split[1]
                        .parse::<i32>()
                        .map_err(|_| ReadError::Parse(line_num))?,
                ),
                CfgVal::U32(_, bounds) => {
                    let val = line_split[1]
                        .parse::<u32>()
                        .map_err(|_| ReadError::Parse(line_num))?;
                    CfgVal::U32(val, update_bounds(bounds, val))
                }
                CfgVal::F32(_, bounds) => {
                    let val = line_split[1]
                        .parse::<f32>()
                        .map_err(|_| ReadError::Parse(line_num))?;
                    CfgVal::F32(val, update_bounds(bounds, val))
                }
            };
            values.push(value);
        }
        Ok(Config::new(values))
    }

    pub fn get(&self, key: CfgKey) -> &CfgVal {
        self.map.get(&key).unwrap()
    }

    pub fn set(&mut self, key: CfgKey, val: CfgVal) -> CfgVal {
        self.map.insert(key, val).unwrap()
    }
}

fn update_bounds<T>(bounds: RangeInclusive<T>, val: T) -> RangeInclusive<T>
where
    T: PartialOrd + Copy,
{
    let mut new_start = *bounds.start();
    let mut new_end = *bounds.end();
    if val < new_start {
        new_start = val;
    } else if val > new_end {
        new_end = val;
    };
    RangeInclusive::new(new_start, new_end)
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
            insert_offset += 1
        });
    snake_str
}
