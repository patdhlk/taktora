// Fixture for scripts/check-complexity.sh self-test. NOT part of any crate.
// Intentionally over the cyclomatic bar so the gate must reject it.
#[allow(dead_code)]
pub fn deliberately_complex(x: i32) -> i32 {
    let mut acc = 0;
    if x == 0 { acc += 1; } else if x == 1 { acc += 2; } else if x == 2 { acc += 3; }
    else if x == 3 { acc += 4; } else if x == 4 { acc += 5; } else if x == 5 { acc += 6; }
    else if x == 6 { acc += 7; } else if x == 7 { acc += 8; } else if x == 8 { acc += 9; }
    else if x == 9 { acc += 10; } else if x == 10 { acc += 11; } else if x == 11 { acc += 12; }
    else if x == 12 { acc += 13; } else if x == 13 { acc += 14; } else if x == 14 { acc += 15; }
    else if x == 15 { acc += 16; } else if x == 16 { acc += 17; } else if x == 17 { acc += 18; }
    else if x == 18 { acc += 19; } else if x == 19 { acc += 20; } else if x == 20 { acc += 21; }
    else if x == 21 { acc += 22; } else { acc += 99; }
    if acc > 10 && x > 0 { acc += 1; }
    acc
}
