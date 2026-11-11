//! Preserve CPython 3.9's comparison order when a legacy key contains NaN.
//! A total-order Rust sort is not equivalent for this accepted JSON dialect.
//!
//! Adapted algorithm: CPython v3.9.6 Objects/listobject.c (Tim Peters' listsort).
//! Copyright (c) 2001-2021 Python Software Foundation. All rights reserved.
//! Licensed under the PSF license; see ../CPython-LICENSE and docs/stats-rust.md.
//! This adaptation sorts indices, uses safe slices, and propagates report errors.
use crate::py::Result;

pub fn sort(n: usize, less: impl Fn(usize, usize) -> Result<bool>) -> Result<Vec<usize>> {
    let mut s = Sort {
        order: (0..n).collect(),
        runs: vec![],
        gallop: 7,
        less,
    };
    let mut m = n;
    let mut r = 0;
    while m >= 64 {
        r |= m & 1;
        m >>= 1;
    }
    let minrun = m + r;
    let mut at = 0;
    while at < n {
        let mut count = 1;
        if at + 1 < n {
            let down = (s.less)(s.order[at + 1], s.order[at])?;
            count = 2;
            while at + count < n {
                let next = (s.less)(s.order[at + count], s.order[at + count - 1])?;
                if next != down {
                    break;
                }
                count += 1;
            }
            if down {
                s.order[at..at + count].reverse();
            }
        }
        let force = minrun.min(n - at);
        if count < force {
            for i in at + count..at + force {
                let pivot = s.order[i];
                let (mut lo, mut hi) = (at, i);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if (s.less)(pivot, s.order[mid])? {
                        hi = mid;
                    } else {
                        lo = mid + 1;
                    }
                }
                s.order.copy_within(lo..i, lo + 1);
                s.order[lo] = pivot;
            }
            count = force;
        }
        s.runs.push((at, count));
        at += count;
        while s.runs.len() > 1 {
            let mut i = s.runs.len() - 2;
            if (i > 0 && s.runs[i - 1].1 <= s.runs[i].1 + s.runs[i + 1].1)
                || (i > 1 && s.runs[i - 2].1 <= s.runs[i - 1].1 + s.runs[i].1)
            {
                if s.runs[i - 1].1 < s.runs[i + 1].1 {
                    i -= 1;
                }
            } else if s.runs[i].1 > s.runs[i + 1].1 {
                break;
            }
            s.merge(i)?;
        }
    }
    while s.runs.len() > 1 {
        let mut i = s.runs.len() - 2;
        if i > 0 && s.runs[i - 1].1 < s.runs[i + 1].1 {
            i -= 1;
        }
        s.merge(i)?;
    }
    Ok(s.order)
}
struct Sort<F> {
    order: Vec<usize>,
    runs: Vec<(usize, usize)>,
    gallop: usize,
    less: F,
}
impl<F: Fn(usize, usize) -> Result<bool>> Sort<F> {
    fn search(&self, key: usize, a: &[usize], hint: usize, right: bool) -> Result<usize> {
        let before = |i| {
            if right {
                (self.less)(key, a[i]).map(|v| !v)
            } else {
                (self.less)(a[i], key)
            }
        };
        let (mut last, mut offset) = (0isize, 1isize);
        let h = hint as isize;
        if before(hint)? {
            let max = a.len() as isize - h;
            while offset < max && before((h + offset) as usize)? {
                last = offset;
                offset = offset * 2 + 1;
            }
            offset = offset.min(max);
            last += h;
            offset += h;
        } else {
            let max = h + 1;
            while offset < max && !before((h - offset) as usize)? {
                last = offset;
                offset = offset * 2 + 1;
            }
            offset = offset.min(max);
            (last, offset) = (h - offset, h - last);
        }
        last += 1;
        while last < offset {
            let mid = last + (offset - last) / 2;
            if before(mid as usize)? {
                last = mid + 1;
            } else {
                offset = mid;
            }
        }
        Ok(offset as usize)
    }
    fn merge(&mut self, i: usize) -> Result<()> {
        let (mut a, mut na) = self.runs[i];
        let (b, mut nb) = self.runs[i + 1];
        self.runs[i].1 += nb;
        self.runs.remove(i + 1);
        let skip = self.search(self.order[b], &self.order[a..a + na], 0, true)?;
        a += skip;
        na -= skip;
        if na == 0 {
            return Ok(());
        }
        nb = self.search(
            self.order[a + na - 1],
            &self.order[b..b + nb],
            nb - 1,
            false,
        )?;
        if nb == 0 {
            return Ok(());
        }
        if na <= nb {
            self.low(a, na, b, nb)
        } else {
            self.high(a, na, b, nb)
        }
    }
    fn low(&mut self, a: usize, mut na: usize, b: usize, mut nb: usize) -> Result<()> {
        let temp = self.order[a..a + na].to_vec();
        let (mut ia, mut ib, mut dest) = (0, b, a);
        self.order[dest] = self.order[ib];
        dest += 1;
        ib += 1;
        nb -= 1;
        let mut min = self.gallop;
        'outer: while nb > 0 && na > 1 {
            let (mut ac, mut bc) = (0, 0);
            loop {
                if (self.less)(self.order[ib], temp[ia])? {
                    self.order[dest] = self.order[ib];
                    dest += 1;
                    ib += 1;
                    nb -= 1;
                    bc += 1;
                    ac = 0;
                    if nb == 0 {
                        break 'outer;
                    }
                    if bc >= min {
                        break;
                    }
                } else {
                    self.order[dest] = temp[ia];
                    dest += 1;
                    ia += 1;
                    na -= 1;
                    ac += 1;
                    bc = 0;
                    if na == 1 {
                        break 'outer;
                    }
                    if ac >= min {
                        break;
                    }
                }
            }
            min += 1;
            loop {
                if min > 1 {
                    min -= 1;
                }
                self.gallop = min;
                ac = self.search(self.order[ib], &temp[ia..ia + na], 0, true)?;
                if ac > 0 {
                    self.order[dest..dest + ac].copy_from_slice(&temp[ia..ia + ac]);
                    dest += ac;
                    ia += ac;
                    na -= ac;
                    if na <= 1 {
                        break 'outer;
                    }
                }
                self.order[dest] = self.order[ib];
                dest += 1;
                ib += 1;
                nb -= 1;
                if nb == 0 {
                    break 'outer;
                }
                bc = self.search(temp[ia], &self.order[ib..ib + nb], 0, false)?;
                if bc > 0 {
                    self.order.copy_within(ib..ib + bc, dest);
                    dest += bc;
                    ib += bc;
                    nb -= bc;
                    if nb == 0 {
                        break 'outer;
                    }
                }
                self.order[dest] = temp[ia];
                dest += 1;
                ia += 1;
                na -= 1;
                if na == 1 {
                    break 'outer;
                }
                if ac < 7 && bc < 7 {
                    break;
                }
            }
            min += 1;
            self.gallop = min;
        }
        if na == 1 && nb > 0 {
            self.order.copy_within(ib..ib + nb, dest);
            self.order[dest + nb] = temp[ia];
        } else if na > 0 {
            self.order[dest..dest + na].copy_from_slice(&temp[ia..ia + na]);
        }
        Ok(())
    }
    fn high(&mut self, a: usize, mut na: usize, b: usize, mut nb: usize) -> Result<()> {
        let temp = self.order[b..b + nb].to_vec();
        let (mut ia, mut ib, mut dest) = (a + na, nb, b + nb);
        dest -= 1;
        ia -= 1;
        self.order[dest] = self.order[ia];
        na -= 1;
        let mut min = self.gallop;
        'outer: while na > 0 && nb > 1 {
            let (mut ac, mut bc) = (0, 0);
            loop {
                if (self.less)(temp[ib - 1], self.order[ia - 1])? {
                    dest -= 1;
                    ia -= 1;
                    self.order[dest] = self.order[ia];
                    na -= 1;
                    ac += 1;
                    bc = 0;
                    if na == 0 {
                        break 'outer;
                    }
                    if ac >= min {
                        break;
                    }
                } else {
                    dest -= 1;
                    ib -= 1;
                    self.order[dest] = temp[ib];
                    nb -= 1;
                    bc += 1;
                    ac = 0;
                    if nb == 1 {
                        break 'outer;
                    }
                    if bc >= min {
                        break;
                    }
                }
            }
            min += 1;
            loop {
                if min > 1 {
                    min -= 1;
                }
                self.gallop = min;
                ac = na - self.search(temp[ib - 1], &self.order[a..a + na], na - 1, true)?;
                if ac > 0 {
                    dest -= ac;
                    ia -= ac;
                    self.order.copy_within(ia..ia + ac, dest);
                    na -= ac;
                    if na == 0 {
                        break 'outer;
                    }
                }
                dest -= 1;
                ib -= 1;
                self.order[dest] = temp[ib];
                nb -= 1;
                if nb == 1 {
                    break 'outer;
                }
                bc = nb - self.search(self.order[ia - 1], &temp[..nb], nb - 1, false)?;
                if bc > 0 {
                    dest -= bc;
                    ib -= bc;
                    self.order[dest..dest + bc].copy_from_slice(&temp[ib..ib + bc]);
                    nb -= bc;
                    if nb <= 1 {
                        break 'outer;
                    }
                }
                dest -= 1;
                ia -= 1;
                self.order[dest] = self.order[ia];
                na -= 1;
                if na == 0 {
                    break 'outer;
                }
                if ac < 7 && bc < 7 {
                    break;
                }
            }
            min += 1;
            self.gallop = min;
        }
        if nb == 1 && na > 0 {
            dest -= na;
            self.order.copy_within(ia - na..ia, dest);
            self.order[dest - 1] = temp[ib - 1];
        } else if nb > 0 {
            self.order[dest - nb..dest].copy_from_slice(&temp[..nb]);
        }
        Ok(())
    }
}
