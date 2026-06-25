// musl as a whole is licensed under the following standard MIT license:
//
// ----------------------------------------------------------------------
// Copyright © 2005-2020 Rich Felker, et al.
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
// ----------------------------------------------------------------------
//
// Authors/contributors include:
//
// A. Wilcox
// Ada Worcester
// Alex Dowad
// Alex Suykov
// Alexander Monakov
// Andre McCurdy
// Andrew Kelley
// Anthony G. Basile
// Aric Belsito
// Arvid Picciani
// Bartosz Brachaczek
// Benjamin Peterson
// Bobby Bingham
// Boris Brezillon
// Brent Cook
// Chris Spiegel
// Clément Vasseur
// Daniel Micay
// Daniel Sabogal
// Daurnimator
// David Carlier
// David Edelsohn
// Denys Vlasenko
// Dmitry Ivanov
// Dmitry V. Levin
// Drew DeVault
// Emil Renner Berthing
// Fangrui Song
// Felix Fietkau
// Felix Janda
// Gianluca Anzolin
// Hauke Mehrtens
// He X
// Hiltjo Posthuma
// Isaac Dunham
// Jaydeep Patil
// Jens Gustedt
// Jeremy Huntwork
// Jo-Philipp Wich
// Joakim Sindholt
// John Spencer
// Julien Ramseier
// Justin Cormack
// Kaarle Ritvanen
// Khem Raj
// Kylie McClain
// Leah Neukirchen
// Luca Barbato
// Luka Perkov
// M Farkas-Dyck (Strake)
// Mahesh Bodapati
// Markus Wichmann
// Masanori Ogino
// Michael Clark
// Michael Forney
// Mikhail Kremnyov
// Natanael Copa
// Nicholas J. Kain
// orc
// Pascal Cuoq
// Patrick Oppenlander
// Petr Hosek
// Petr Skocik
// Pierre Carrier
// Reini Urban
// Rich Felker
// Richard Pennington
// Ryan Fairfax
// Samuel Holland
// Segev Finer
// Shiz
// sin
// Solar Designer
// Stefan Kristiansson
// Stefan O'Rear
// Szabolcs Nagy
// Timo Teräs
// Trutz Behn
// Valentin Ochs
// Will Dietz
// William Haddon
// William Pitcock
//
// Portions of this software are derived from third-party works licensed
// under terms compatible with the above MIT license:
//
// The TRE regular expression implementation (src/regex/reg* and
// src/regex/tre*) is Copyright © 2001-2008 Ville Laurikari and licensed
// under a 2-clause BSD license (license text in the source files). The
// included version has been heavily modified by Rich Felker in 2012, in
// the interests of size, simplicity, and namespace cleanliness.
//
// Much of the math library code (src/math/* and src/complex/*) is
// Copyright © 1993,2004 Sun Microsystems or
// Copyright © 2003-2011 David Schultz or
// Copyright © 2003-2009 Steven G. Kargl or
// Copyright © 2003-2009 Bruce D. Evans or
// Copyright © 2008 Stephen L. Moshier or
// Copyright © 2017-2018 Arm Limited
// and labelled as such in comments in the individual source files. All
// have been licensed under extremely permissive terms.
//
// The ARM memcpy code (src/string/arm/memcpy.S) is Copyright © 2008
// The Android Open Source Project and is licensed under a two-clause BSD
// license. It was taken from Bionic libc, used on Android.
//
// The AArch64 memcpy and memset code (src/string/aarch64/*) are
// Copyright © 1999-2019, Arm Limited.
//
// The implementation of DES for crypt (src/crypt/crypt_des.c) is
// Copyright © 1994 David Burren. It is licensed under a BSD license.
//
// The implementation of blowfish crypt (src/crypt/crypt_blowfish.c) was
// originally written by Solar Designer and placed into the public
// domain. The code also comes with a fallback permissive license for use
// in jurisdictions that may not recognize the public domain.
//
// The smoothsort implementation (src/stdlib/qsort.c) is Copyright © 2011
// Valentin Ochs and is licensed under an MIT-style license.
//
// The x86_64 port was written by Nicholas J. Kain and is licensed under
// the standard MIT terms.
//
// The mips and microblaze ports were originally written by Richard
// Pennington for use in the ellcc project. The original code was adapted
// by Rich Felker for build system and code conventions during upstream
// integration. It is licensed under the standard MIT terms.
//
// The mips64 port was contributed by Imagination Technologies and is
// licensed under the standard MIT terms.
//
// The powerpc port was also originally written by Richard Pennington,
// and later supplemented and integrated by John Spencer. It is licensed
// under the standard MIT terms.
//
// All other files which have no copyright comments are original works
// produced specifically for use as part of this library, written either
// by Rich Felker, the main author of the library, or by one or more
// contibutors listed above. Details on authorship of individual files
// can be found in the git version control history of the project. The
// omission of copyright and license comments in each file is in the
// interest of source tree size.
//
// In addition, permission is hereby granted for all public header files
// (include/* and arch/*/bits/*) and crt files intended to be linked into
// applications (crt/*, ldso/dlstart.c, and arch/*/crt_arch.h) to omit
// the copyright notice and permission notice otherwise required by the
// license, and to use these files without any requirement of
// attribution. These files include substantial contributions from:
//
// Bobby Bingham
// John Spencer
// Nicholas J. Kain
// Rich Felker
// Richard Pennington
// Stefan Kristiansson
// Szabolcs Nagy
//
// all of whom have explicitly granted such permission.
//
// This file previously contained text expressing a belief that most of
// the files covered by the above exception were sufficiently trivial not
// to be subject to copyright, resulting in confusion over whether it
// negated the permissions granted in the license. In the spirit of
// permissive licensing, and of not having licensing issues being an
// obstacle to adoption, that text has been removed.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Nanoseconds in one whole second.
const NANOS_PER_SECOND: u32 = 1_000_000_000;
/// Integer divisor that converts nanoseconds to microseconds for display.
const MICROS_PER_NANO_DIVISOR: u32 = 1_000;
/// Seconds in one minute.
const SECONDS_PER_MINUTE: i32 = 60;
/// Seconds in one hour.
const SECONDS_PER_HOUR: i32 = 3_600;
/// Seconds in one day.
const SECONDS_PER_DAY: i64 = 86_400;
/// Day index of 2000-03-01, immediately after February 29 in the 400-year cycle.
const LEAPOCH_DAYS: i64 = 11_017;
/// Days in one Gregorian 400-year cycle.
const DAYS_PER_400_YEARS: i32 = 146_097;
/// Days in one Gregorian 100-year cycle within the 400-year cycle.
const DAYS_PER_100_YEARS: i32 = 36_524;
/// Days in one Gregorian four-year cycle.
const DAYS_PER_4_YEARS: i32 = 1_461;
/// Days in one non-leap year.
const DAYS_PER_YEAR: i32 = 365;
/// Month lengths from March through February in leap-year order.
const DAYS_IN_MONTH: [i8; 12] = [31, 30, 31, 30, 31, 31, 30, 31, 30, 31, 31, 29];

/// A date/time type which exists primarily to convert `SystemTime` timestamps into an ISO 8601
/// formatted string.
///
/// Yes, this exists. Before you have a heart attack, understand that the meat of this is musl's
/// [`__secs_to_tm`][1] converted to Rust via [c2rust][2] and then cleaned up by hand as part of
/// the [kudu-rs project][3], [released under MIT][4].
///
/// [1] <http://git.musl-libc.org/cgit/musl/tree/src/time>/__`secs_to_tm.c`
/// [2] <https://c2rust.com>/
/// [3] <https://github.com/danburkert/kudu-rs/blob/c9660067e5f4c1a54143f169b5eeb49446f82e54/src/timestamp.rs#L5-L18>
/// [4] <https://github.com/tokio-rs/tracing/issues/1644#issuecomment-963888244>
///
/// All existing `strftime`-like APIs I found were unable to handle the full range of timestamps representable
/// by `SystemTime`, including `strftime` itself, since `tm.tm_year` is an int.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DateTime {
    /// The astronomical calendar year.
    year: i64,
    /// The one-indexed calendar month.
    month: u8,
    /// The one-indexed day within the calendar month.
    day: u8,
    /// The zero-indexed hour within the day.
    hour: u8,
    /// The zero-indexed minute within the hour.
    minute: u8,
    /// The zero-indexed second within the minute.
    second: u8,
    /// The nanosecond component within the second.
    nanos: u32,
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.year > 9999 {
            write!(f, "+{}", self.year)?;
        } else if self.year < 0 {
            write!(f, "{:05}", self.year)?;
        } else {
            write!(f, "{:04}", self.year)?;
        }

        write!(
            f,
            "-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.nanos.div_euclid(MICROS_PER_NANO_DIVISOR)
        )
    }
}

impl From<SystemTime> for DateTime {
    fn from(timestamp: SystemTime) -> Self {
        let (timestamp_secs, nanos) = match timestamp.duration_since(UNIX_EPOCH) {
            Ok(duration) => (duration_seconds_i64(duration), duration.subsec_nanos()),
            Err(error) => {
                let duration = error.duration();
                let secs = duration_seconds_i64(duration);
                let nanos = duration.subsec_nanos();
                if nanos == 0 {
                    (secs.saturating_neg(), 0)
                } else {
                    (
                        secs.saturating_neg().saturating_sub(1),
                        NANOS_PER_SECOND.saturating_sub(nanos),
                    )
                }
            }
        };

        let days_since_leapoch = timestamp_secs
            .div_euclid(SECONDS_PER_DAY)
            .saturating_sub(LEAPOCH_DAYS);
        let remaining_secs =
            i32::try_from(timestamp_secs.rem_euclid(SECONDS_PER_DAY)).unwrap_or_default();

        let quadricentennial_cycles = i32::try_from(
            days_since_leapoch.div_euclid(i64::from(DAYS_PER_400_YEARS)),
        )
        .unwrap_or_default();
        let mut remaining_days = i32::try_from(
            days_since_leapoch.rem_euclid(i64::from(DAYS_PER_400_YEARS)),
        )
        .unwrap_or_default();

        let century_cycles = clamped_cycle_count(remaining_days, DAYS_PER_100_YEARS, 4);
        remaining_days = subtract_cycle_days(remaining_days, century_cycles, DAYS_PER_100_YEARS);

        let quadrennial_cycles = clamped_cycle_count(remaining_days, DAYS_PER_4_YEARS, 25);
        remaining_days =
            subtract_cycle_days(remaining_days, quadrennial_cycles, DAYS_PER_4_YEARS);

        let remaining_years = clamped_cycle_count(remaining_days, DAYS_PER_YEAR, 4);
        remaining_days = subtract_cycle_days(remaining_days, remaining_years, DAYS_PER_YEAR);

        let mut years = i64::from(remaining_years)
            .saturating_add(i64::from(quadrennial_cycles).saturating_mul(4))
            .saturating_add(i64::from(century_cycles).saturating_mul(100))
            .saturating_add(i64::from(quadricentennial_cycles).saturating_mul(400));

        let mut month_index = 0_i32;
        for month_length_i8 in DAYS_IN_MONTH {
            let month_length = i32::from(month_length_i8);
            if month_length > remaining_days {
                break;
            }

            remaining_days = remaining_days.saturating_sub(month_length);
            month_index = month_index.saturating_add(1);
        }

        if month_index >= 10 {
            month_index = month_index.saturating_sub(12);
            years = years.saturating_add(1);
        }

        Self {
            year: years.saturating_add(2000),
            month: u8::try_from(month_index.saturating_add(3)).unwrap_or_default(),
            day: u8::try_from(remaining_days.saturating_add(1)).unwrap_or_default(),
            hour: u8::try_from(remaining_secs.div_euclid(SECONDS_PER_HOUR)).unwrap_or_default(),
            minute: u8::try_from(
                remaining_secs
                    .rem_euclid(SECONDS_PER_HOUR)
                    .div_euclid(SECONDS_PER_MINUTE),
            )
            .unwrap_or_default(),
            second: u8::try_from(remaining_secs.rem_euclid(SECONDS_PER_MINUTE))
                .unwrap_or_default(),
            nanos,
        }
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::DateTime<chrono::Utc>> for DateTime {
    fn from(timestamp: chrono::DateTime<chrono::Utc>) -> Self {
        use chrono::{Datelike as _, Timelike as _};

        Self {
            year: i64::from(timestamp.year()),
            month: u8::try_from(timestamp.month()).unwrap_or_default(),
            day: u8::try_from(timestamp.day()).unwrap_or_default(),
            hour: u8::try_from(timestamp.hour()).unwrap_or_default(),
            minute: u8::try_from(timestamp.minute()).unwrap_or_default(),
            second: u8::try_from(timestamp.second()).unwrap_or_default(),
            nanos: timestamp.timestamp_subsec_nanos(),
        }
    }
}

/// Converts a [`Duration`] second count into the signed range used by this converter.
fn duration_seconds_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}

/// Counts calendar cycles, clamping the exclusive boundary case from musl's algorithm.
const fn clamped_cycle_count(
    remaining_days: i32,
    cycle_days: i32,
    exclusive_upper_bound: i32,
) -> i32 {
    let cycles = remaining_days.div_euclid(cycle_days);
    if cycles == exclusive_upper_bound {
        cycles.saturating_sub(1)
    } else {
        cycles
    }
}

/// Removes the days represented by complete calendar cycles.
const fn subtract_cycle_days(remaining_days: i32, cycles: i32, cycle_days: i32) -> i32 {
    remaining_days.saturating_sub(cycles.saturating_mul(cycle_days))
}

#[cfg(test)]
mod tests {
    use i32;
    use std::{
        format,
        time::{Duration, UNIX_EPOCH},
    };

    use super::*;
    use strict_test_support::{TestFailure, ensure};

    #[test]
    fn test_datetime() -> Result<(), TestFailure> {
        let case = |expected: &str, secs: i64, micros: u32| -> Result<(), TestFailure> {
            let timestamp = if secs >= 0 {
                UNIX_EPOCH + Duration::new(secs.cast_unsigned(), micros * 1_000)
            } else {
                (UNIX_EPOCH - Duration::new((!secs).cast_unsigned() + 1, 0))
                    + Duration::new(0, micros * 1_000)
            };
            ensure(
                expected == format!("{}", DateTime::from(timestamp)),
                "datetime renders expected timestamp",
            )
        };

        // Mostly generated with:
        //  - date -jur <secs> +"%Y-%m-%dT%H:%M:%S.000000Z"
        //  - http://unixtimestamp.50x.eu/

        case("1970-01-01T00:00:00.000000Z", 0, 0)?;

        case("1970-01-01T00:00:00.000001Z", 0, 1)?;
        case("1970-01-01T00:00:00.500000Z", 0, 500_000)?;
        case("1970-01-01T00:00:01.000001Z", 1, 1)?;
        case("1970-01-01T00:01:01.000001Z", 60 + 1, 1)?;
        case("1970-01-01T01:01:01.000001Z", 60 * 60 + 60 + 1, 1)?;
        case(
            "1970-01-02T01:01:01.000001Z",
            24 * 60 * 60 + 60 * 60 + 60 + 1,
            1,
        )?;

        case("1969-12-31T23:59:59.000000Z", -1, 0)?;
        case("1969-12-31T23:59:59.000001Z", -1, 1)?;
        case("1969-12-31T23:59:59.500000Z", -1, 500_000)?;
        case("1969-12-31T23:58:59.000001Z", -60 - 1, 1)?;
        case("1969-12-31T22:58:59.000001Z", -60 * 60 - 60 - 1, 1)?;
        case(
            "1969-12-30T22:58:59.000001Z",
            -24 * 60 * 60 - 60 * 60 - 60 - 1,
            1,
        )?;

        case("2038-01-19T03:14:07.000000Z", i64::from(i32::MAX), 0)?;
        case("2038-01-19T03:14:08.000000Z", i64::from(i32::MAX) + 1, 0)?;
        case("1901-12-13T20:45:52.000000Z", i64::from(i32::MIN), 0)?;
        case("1901-12-13T20:45:51.000000Z", i64::from(i32::MIN) - 1, 0)?;

        // Skipping these tests on windows as std::time::SystemTime range is low
        // on Windows compared with that of Unix which can cause the following
        // high date value tests to panic
        #[cfg(not(target_os = "windows"))]
        {
            case("+292277026596-12-04T15:30:07.000000Z", i64::MAX, 0)?;
            case("+292277026596-12-04T15:30:06.000000Z", i64::MAX - 1, 0)?;
            case("-292277022657-01-27T08:29:53.000000Z", i64::MIN + 1, 0)
        }?;

        case("1900-01-01T00:00:00.000000Z", -2_208_988_800, 0)?;
        case("1899-12-31T23:59:59.000000Z", -2_208_988_801, 0)?;
        case("2345-06-07T08:09:01.000000Z", 11_847_456_541, 0)?;

        // Skipping pre-1601 dates on Windows: as of Rust 1.94, SystemTime
        // subtraction panics when the result would be before the Windows
        // FILETIME epoch (1601-01-01). See Rust 1.94.0 compatibility notes.
        #[cfg(not(target_os = "windows"))]
        {
            case("1234-05-06T07:08:09.000000Z", -23_215_049_511, 0)?;
            case("0000-01-01T00:00:00.000000Z", -62_167_219_200, 0)?;
            case("-0001-12-31T23:59:59.000000Z", -62_167_219_201, 0)?;
            case("-1234-05-06T07:08:09.000000Z", -101_097_651_111, 0)?;
            case("-2345-06-07T08:09:01.000000Z", -136_154_620_259, 0)
        }?;
        Ok(())
    }
}
