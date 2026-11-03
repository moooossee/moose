use serde_json::Value;

type Result<T> = std::result::Result<T, String>;

pub(super) fn calculate(arguments: &Value) -> Result<String> {
    let object = arguments
        .as_object()
        .ok_or("Provide an object with an expression string.")?;
    if object.len() != 1 {
        return Err("Provide only the expression argument.".into());
    }
    let expression = object
        .get("expression")
        .and_then(Value::as_str)
        .ok_or("Provide an expression string.")?;
    if expression.trim().is_empty() || expression.len() > 512 {
        return Err("Use an expression between 1 and 512 bytes long.".into());
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
        return Err("Unexpected text. Use explicit operators and supported functions only.".into());
    }
    if value == 0.0 {
        return Ok("0".into());
    }
    let formatted = format!("{value:.14e}");
    if value.abs() < 1e-9 || value.abs() >= 1e15 {
        let (number, exponent) = formatted
            .split_once('e')
            .ok_or("The result could not be formatted.")?;
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
            return Err(
                "This expression is too complex. Split it into smaller calculations.".into(),
            );
        }
        self.whitespace();
        let mut value = if self.take(b'+') {
            self.expression(5, depth + 1)?
        } else if self.take(b'-') {
            -self.expression(5, depth + 1)?
        } else if self.take(b'(') {
            let value = self.expression(0, depth + 1)?;
            if !self.take(b')') {
                return Err("A closing parenthesis is missing.".into());
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
                b'/' if operand == 0.0 => return Err("Division by zero is not defined.".into()),
                b'/' => value / operand,
                b'^' => value.powf(operand),
                _ => unreachable!(),
            };
            if !value.is_finite() {
                return Err("The result is outside the supported finite real numbers.".into());
            }
        }
        if !value.is_finite() {
            return Err("The result is outside the supported finite real numbers.".into());
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
            return Err("Expected a number, a parenthesis, or a supported function.".into());
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
                return Err("Scientific notation needs an exponent, such as 1e3.".into());
            }
        }
        std::str::from_utf8(&self.input[start..self.position])
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                "Use a finite number with a decimal point and no digit separators.".into()
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
            _ => return Err("Supported functions are sqrt, abs, round, floor, and ceil.".into()),
        }
        if !self.take(b'(') {
            return Err("Function arguments need parentheses.".into());
        }
        let value = self.expression(0, depth + 1)?;
        if !self.take(b')') {
            return Err("A closing parenthesis is missing.".into());
        }
        match name.as_slice() {
            b"sqrt" if value < 0.0 => {
                Err("The square root of a negative number is not real.".into())
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
