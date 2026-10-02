#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandleInterval {
    OneSecond,
    OneMinute,
    ThreeMinutes,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    FourHours,
    SixHours,
    TwelveHours,
    OneDay,
    OneWeek,
}

impl CandleInterval {
    pub fn seconds(self) -> i64 {
        match self {
            Self::OneSecond => 1,
            Self::OneMinute => 60,
            Self::ThreeMinutes => 3 * 60,
            Self::FiveMinutes => 5 * 60,
            Self::FifteenMinutes => 15 * 60,
            Self::ThirtyMinutes => 30 * 60,
            Self::OneHour => 60 * 60,
            Self::FourHours => 4 * 60 * 60,
            Self::SixHours => 6 * 60 * 60,
            Self::TwelveHours => 12 * 60 * 60,
            Self::OneDay => 24 * 60 * 60,
            Self::OneWeek => 7 * 24 * 60 * 60,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneSecond => "1s",
            Self::OneMinute => "1m",
            Self::ThreeMinutes => "3m",
            Self::FiveMinutes => "5m",
            Self::FifteenMinutes => "15m",
            Self::ThirtyMinutes => "30m",
            Self::OneHour => "1h",
            Self::FourHours => "4h",
            Self::SixHours => "6h",
            Self::TwelveHours => "12h",
            Self::OneDay => "1d",
            Self::OneWeek => "1w",
        }
    }

    pub fn bucket_start(self, timestamp: i64) -> i64 {
        let interval_seconds = self.seconds();

        timestamp.div_euclid(interval_seconds) * interval_seconds
    }
}

impl TryFrom<&str> for CandleInterval {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "1s" => Ok(Self::OneSecond),
            "1m" => Ok(Self::OneMinute),
            "3m" => Ok(Self::ThreeMinutes),
            "5m" => Ok(Self::FiveMinutes),
            "15m" => Ok(Self::FifteenMinutes),
            "30m" => Ok(Self::ThirtyMinutes),
            "1h" => Ok(Self::OneHour),
            "4h" => Ok(Self::FourHours),
            "6h" => Ok(Self::SixHours),
            "12h" => Ok(Self::TwelveHours),
            "1d" => Ok(Self::OneDay),
            "1w" => Ok(Self::OneWeek),
            _ => Err(()),
        }
    }
}