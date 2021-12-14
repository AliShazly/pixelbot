extern crate num;
use num_enum::TryFromPrimitive;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;
use std::fmt::Display;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Write;
use std::ops::{Range, RangeInclusive};

const CFG_FILE_PATH: &str = "./config.cfg";

#[derive(Debug)]
pub enum ReadError {
    Parse(u32),
    InvalidKey(u32),
    OutOfBounds(u32),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Parse(line_num) => write!(f, "Parse error on line {}", line_num),
            Self::InvalidKey(line_num) => write!(f, "Invalid key on line {}", line_num),
            Self::OutOfBounds(line_num) => write!(f, "Out of bounds value on line {}", line_num),
        }
    }
}

impl std::error::Error for ReadError {}

#[derive(Debug, Hash, PartialEq, Eq, TryFromPrimitive, Clone, Copy)]
#[repr(usize)]
pub enum CfgKey {
    CropW,
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
    _Size, // Last item get assigned the size of the enum
}
const N_CONFIG_ITEMS: usize = CfgKey::_Size as usize;

impl CfgKey {
    // Uses TryFromPrimitive to convert integer into variant of cfgkey struct
    fn iter() -> impl Iterator<Item = Self> {
        (0..N_CONFIG_ITEMS).map(|i| Self::try_from(i).unwrap())
    }

    fn default_val(&self) -> ValType {
        use CfgKey::*;
        use ValType::*;

        match *self {
            CropW => Unsigned(CfgValue::new(1152, None)), // Bounds set at runtime for crop_w & crop_h
            CropH => Unsigned(CfgValue::new(592, None)),
            ColorThresh => Float(CfgValue::new(0.83, Some(0.0..1.0))),
            AimDivisor => Float(CfgValue::new(3., Some(1.0..10.0))),
            YDivisor => Float(CfgValue::new(1.3, Some(1.0..2.0))),
            Fps => Unsigned(CfgValue::new(144, Some(1..240))),
            MaxAutoclickSleepMs => Unsigned(CfgValue::new(90, Some(0..100))),
            MinAutoclickSleepMs => Unsigned(CfgValue::new(50, Some(0..100))),
            AimDurationMicros => Unsigned(CfgValue::new(50, Some(0..2000))),
            AimSteps => Unsigned(CfgValue::new(2, Some(1..10))),
            AimKeycode => Keycode(CfgValue::new(1, None)),
            AutoclickKeycode => Keycode(CfgValue::new(1, None)),
            ToggleAimKeycode => Keycode(CfgValue::new(190, None)),
            ToggleAutoclickKeycode => Keycode(CfgValue::new(188, None)),
            _ => panic!("Default values not exhaustive"),
        }
    }

    pub fn as_string(&self) -> String {
        camel_to_snake(format!("{:?}", self).as_str())
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct CfgValue<T> {
    pub val: T,
    pub bounds: Option<Range<T>>,
}

impl<T> CfgValue<T> {
    pub fn new(val: T, bounds: Option<Range<T>>) -> Self {
        Self { val, bounds }
    }

    // Allows casting between cfgvalues that would support it
    fn from<U>(other: &CfgValue<U>) -> CfgValue<T>
    where
        U: num::NumCast + Copy,
        T: num::NumCast + Copy,
    {
        CfgValue {
            val: num::cast(other.val).unwrap(),
            bounds: other
                .bounds
                .clone()
                .map(|x| num::cast(x.start).unwrap()..num::cast(x.end).unwrap()),
        }
    }

    fn val_in_bounds(&self, val: T) -> bool
    where
        T: std::cmp::PartialOrd + Copy,
    {
        if let Some(range) = &self.bounds {
            range.contains(&val)
        } else {
            true // no bounds, allow all values
        }
    }
}

#[derive(Debug, PartialEq)]
enum ValType {
    Keycode(CfgValue<i32>),
    Unsigned(CfgValue<u32>),
    Float(CfgValue<f32>),
}

impl Display for ValType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keycode(v) => write!(f, "{}", v.val),
            Self::Unsigned(v) => write!(f, "{}", v.val),
            Self::Float(v) => write!(f, "{}", v.val),
        }
    }
}

#[derive(Debug)]
pub struct Config {
    map: HashMap<CfgKey, ValType>,
}

impl Config {
    fn new(mut map: HashMap<CfgKey, ValType>) -> Self {
        CfgKey::iter().for_each(|key| {
            map.entry(key).or_insert_with(|| key.default_val());
        });
        Self { map }
    }

    pub fn default() -> Self {
        Self::new(CfgKey::iter().map(|key| (key, key.default_val())).collect())
    }

    pub fn get<T>(&self, key: CfgKey) -> CfgValue<T>
    where
        T: num::NumCast + Copy,
    {
        match self.map.get(&key).unwrap() {
            ValType::Keycode(x) => CfgValue::from(x),
            ValType::Unsigned(x) => CfgValue::from(x),
            ValType::Float(x) => CfgValue::from(x),
        }
    }

    pub fn set<T>(
        &mut self,
        key: CfgKey,
        new_val: &CfgValue<T>,
        update_bounds: bool,
    ) -> Result<(), &'static str>
    where
        T: num::NumCast + Copy,
    {
        // Generates the match statement to assign to each of the numerical ValTypes,
        // since each arm would do the exact same thing
        macro_rules! match_assign {
            ($obj:expr, $($matcher:path),*) => {
                match $obj {
                    $($matcher(ref mut val_ref) => {
                        let new_val_cast = CfgValue::from(new_val);
                        if update_bounds {
                            if !new_val_cast.val_in_bounds(new_val_cast.val){
                                return Err("New val out of provided bounds");
                            }
                            val_ref.bounds = new_val_cast.bounds;
                        } else {
                            if !val_ref.val_in_bounds(new_val_cast.val){
                                return Err("New val out of stored bounds");
                            }
                        }
                        val_ref.val = new_val_cast.val;
                    }),*
                }
            }
        }

        let val = self.map.get_mut(&key).unwrap();
        match_assign!(val, ValType::Keycode, ValType::Unsigned, ValType::Float);
        Ok(())
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
        let mut map: HashMap<CfgKey, ValType> = HashMap::new();
        let infile = File::open(CFG_FILE_PATH)?;
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
                ValType::Keycode(_) => ValType::Keycode(CfgValue::new(
                    line_split[1]
                        .parse::<i32>()
                        .map_err(|_| ReadError::Parse(line_num))?,
                    None,
                )),
                ValType::Unsigned(v) => {
                    let val = line_split[1]
                        .parse::<u32>()
                        .map_err(|_| ReadError::Parse(line_num))?;
                    if !v.val_in_bounds(val) {
                        return Err(Box::new(ReadError::OutOfBounds(line_num)));
                    }
                    ValType::Unsigned(CfgValue::new(val, v.bounds))
                }
                ValType::Float(v) => {
                    let val = line_split[1]
                        .parse::<f32>()
                        .map_err(|_| ReadError::Parse(line_num))?;
                    if !v.val_in_bounds(val) {
                        return Err(Box::new(ReadError::OutOfBounds(line_num)));
                    }
                    ValType::Float(CfgValue::new(val, v.bounds))
                }
            };
            map.insert(*key, value);
        }
        Ok(Config::new(map))
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
            insert_offset += 1
        });
    snake_str
}
