use crate::i18n::gettext;

use serde_json::Value;

type Result<T> = std::result::Result<T, String>;

pub(super) fn calculate(arguments: &Value) -> Result<String> {
    let object = arguments
        .as_object()
        .ok_or(&gettext("Provide an object with an expression string."))?;
    if object.len() != 1 {
        return Err(gettext("Provide only the expression argument."));
    }
    let expression = object
        .get("expression")
        .and_then(Value::as_str)
        .ok_or(&gettext("Provide an expression string."))?;
    if expression.trim().is_empty() || expression.len() > 512 {
        return Err(gettext("Use an expression between 1 and 512 bytes long."));
    }
    let expression = expression
        .replace('×', "*")
        .replace('÷', "/")
        .replace('−', "-");
    let mut parser = Parser {
        input: expression.as_bytes(),
        position: 0,
        steps: 0,
    };
    let value = parser.expression(0, 0)?;
    parser.whitespace();
    if parser.position != parser.input.len() {
        return Err(gettext(
            "Unexpected text. Use explicit operators and supported functions only.",
        ));
    }
    if value == 0.0 {
        return Ok("0".into());
    }
    let formatted = format!("{value:.14e}");
    if value.abs() < 1e-9 || value.abs() >= 1e15 {
        let (number, exponent) = formatted
            .split_once('e')
            .ok_or(&gettext("The result could not be formatted."))?;
        return Ok(format!(
            "{}e{exponent}",
            number.trim_end_matches('0').trim_end_matches('.')
        ));
    }
    Ok(formatted
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .unwrap_or(value)
        .to_string())
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    steps: usize,
}

impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        self.whitespace();
        if self.input.get(self.position) == Some(&byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expression(&mut self, minimum: u8, depth: usize) -> Result<f64> {
        self.steps += 1;
        if depth > 32 || self.steps > 256 {
            return Err(gettext(
                "This expression is too complex. Split it into smaller calculations.",
            ));
        }
        self.whitespace();
        let mut value = if self.take(b'+') {
            self.expression(5, depth + 1)?
        } else if self.take(b'-') {
            -self.expression(5, depth + 1)?
        } else if self.take(b'(') {
            let value = self.expression(0, depth + 1)?;
            if !self.take(b')') {
                return Err(gettext("A closing parenthesis is missing."));
            }
            value
        } else if self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            self.function(depth + 1)?
        } else {
            self.number()?
        };
        loop {
            self.whitespace();
            let Some(&operator) = self.input.get(self.position) else {
                break;
            };
            if operator == b'%' && minimum <= 7 {
                self.position += 1;
                value /= 100.0;
                continue;
            }
            let (left, right) = match operator {
                b'+' | b'-' => (1, 2),
                b'*' | b'/' => (3, 4),
                b'^' => (6, 5),
                _ => break,
            };
            if left < minimum {
                break;
            }
            self.position += 1;
            let operand = self.expression(right, depth + 1)?;
            value = match operator {
                b'+' => value + operand,
                b'-' => value - operand,
                b'*' => value * operand,
                b'/' if operand == 0.0 => return Err(gettext("Division by zero is not defined.")),
                b'/' => value / operand,
                b'^' => value.powf(operand),
                _ => unreachable!(),
            };
            if !value.is_finite() {
                return Err(gettext(
                    "The result is outside the supported finite real numbers.",
                ));
            }
        }
        if !value.is_finite() {
            return Err(gettext(
                "The result is outside the supported finite real numbers.",
            ));
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<f64> {
        let start = self.position;
        let mut digits = 0;
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
            digits += 1;
        }
        if self.input.get(self.position) == Some(&b'.') {
            self.position += 1;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(gettext(
                "Expected a number, a parenthesis, or a supported function.",
            ));
        }
        if matches!(self.input.get(self.position), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.input.get(self.position), Some(b'+' | b'-')) {
                self.position += 1;
            }
            let exponent = self.position;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if exponent == self.position {
                return Err(gettext(
                    "Scientific notation needs an exponent, such as 1e3.",
                ));
            }
        }
        std::str::from_utf8(&self.input[start..self.position])
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                gettext("Use a finite number with a decimal point and no digit separators.")
            })
    }

    fn function(&mut self, depth: usize) -> Result<f64> {
        let start = self.position;
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            self.position += 1;
        }
        let name = self.input[start..self.position].to_vec();
        match name.as_slice() {
            b"pi" => return Ok(std::f64::consts::PI),
            b"e" => return Ok(std::f64::consts::E),
            b"sqrt" | b"abs" | b"round" | b"floor" | b"ceil" => {}
            _ => {
                return Err(gettext(
                    "Supported functions are sqrt, abs, round, floor, and ceil.",
                ));
            }
        }
        if !self.take(b'(') {
            return Err(gettext("Function arguments need parentheses."));
        }
        let value = self.expression(0, depth + 1)?;
        if !self.take(b')') {
            return Err(gettext("A closing parenthesis is missing."));
        }
        match name.as_slice() {
            b"sqrt" if value < 0.0 => {
                Err(gettext("The square root of a negative number is not real."))
            }
            b"sqrt" => Ok(value.sqrt()),
            b"abs" => Ok(value.abs()),
            b"round" => Ok(value.round()),
            b"floor" => Ok(value.floor()),
            b"ceil" => Ok(value.ceil()),
            _ => unreachable!(),
        }
    }
}
