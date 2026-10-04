//! Persisted presentation preference; never part of a document locator.
use std::io;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PageMode {
    #[default]
    Slide,
    Book,
    Scroll,
}
impl PageMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Slide => "slide",
            Self::Book => "book",
            Self::Scroll => "scroll",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Slide => "左右滑动",
            Self::Book => "仿书翻页",
            Self::Scroll => "上下平滑滚动",
        }
    }
    pub(crate) fn parse(value: &str) -> io::Result<Self> {
        match value {
            "slide" => Ok(Self::Slide),
            "book" => Ok(Self::Book),
            "scroll" => Ok(Self::Scroll),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "page-mode expects slide, book or scroll",
            )),
        }
    }
    pub(crate) fn step(self, direction: i32) -> Self {
        let index = match self {
            Self::Slide => 0_i32,
            Self::Book => 1,
            Self::Scroll => 2,
        };
        [Self::Slide, Self::Book, Self::Scroll][(index + direction.signum()).rem_euclid(3) as usize]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_round_trip_and_cycle_both_directions() {
        for mode in [PageMode::Slide, PageMode::Book, PageMode::Scroll] {
            assert_eq!(PageMode::parse(mode.name()).unwrap(), mode);
            assert_eq!(mode.step(1).step(-1), mode);
            assert_eq!(mode.step(1).step(1).step(1), mode);
        }
        assert!(PageMode::parse("unknown").is_err());
    }
}
