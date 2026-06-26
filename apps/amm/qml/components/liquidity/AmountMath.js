.pragma library

var base58Alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
var U128_MAX = "340282366920938463463374607431768211455"
var U32_MAX = "4294967295"
var Q64 = "18446744073709551616"

function normalize(value) {
    var text = String(value === undefined || value === null ? "" : value)
    var index = 0
    while (index + 1 < text.length && text.charAt(index) === "0")
        ++index
    return text.length > 0 ? text.slice(index) : "0"
}

function isUnsigned(value) {
    return /^[0-9]+$/.test(String(value))
}

function compare(left, right) {
    var a = normalize(left)
    var b = normalize(right)
    if (a.length !== b.length)
        return a.length < b.length ? -1 : 1
    if (a === b)
        return 0
    return a < b ? -1 : 1
}

function compareBase58Ids(left, right) {
    var leftStart = 0
    var rightStart = 0
    while (leftStart < left.length && left[leftStart] === "1")
        ++leftStart
    while (rightStart < right.length && right[rightStart] === "1")
        ++rightStart

    var leftLength = left.length - leftStart
    var rightLength = right.length - rightStart
    if (leftLength !== rightLength)
        return leftLength < rightLength ? -1 : 1

    for (var i = 0; i < leftLength; ++i) {
        var leftDigit = base58Alphabet.indexOf(left[leftStart + i])
        var rightDigit = base58Alphabet.indexOf(right[rightStart + i])
        if (leftDigit < 0 || rightDigit < 0)
            return 0
        if (leftDigit !== rightDigit)
            return leftDigit < rightDigit ? -1 : 1
    }
    return 0
}

function subtract(left, right) {
    var a = normalize(left)
    var b = normalize(right)
    var result = []
    var borrow = 0
    var j = b.length - 1
    for (var i = a.length - 1; i >= 0; --i) {
        var digit = Number(a.charAt(i)) - borrow - (j >= 0 ? Number(b.charAt(j)) : 0)
        if (digit < 0) {
            digit += 10
            borrow = 1
        } else {
            borrow = 0
        }
        result.unshift(String(digit))
        --j
    }
    return normalize(result.join(""))
}

function multiply(left, right) {
    var a = normalize(left)
    var b = normalize(right)
    if (a === "0" || b === "0")
        return "0"

    var result = []
    for (var z = 0; z < a.length + b.length; ++z)
        result.push(0)

    for (var i = a.length - 1; i >= 0; --i) {
        for (var j = b.length - 1; j >= 0; --j) {
            var low = i + j + 1
            var high = i + j
            var product = Number(a.charAt(i)) * Number(b.charAt(j)) + result[low]
            result[low] = product % 10
            result[high] += Math.floor(product / 10)
        }
    }
    return normalize(result.join(""))
}

function divide(left, right) {
    var numerator = normalize(left)
    var denominator = normalize(right)
    if (denominator === "0")
        return { "quotient": "0", "remainder": numerator, "valid": false }

    var quotient = ""
    var remainder = "0"
    for (var i = 0; i < numerator.length; ++i) {
        remainder = normalize((remainder === "0" ? "" : remainder) + numerator.charAt(i))
        var digit = 0
        while (compare(remainder, denominator) >= 0) {
            remainder = subtract(remainder, denominator)
            ++digit
        }
        quotient += String(digit)
    }
    return { "quotient": normalize(quotient), "remainder": remainder, "valid": true }
}

function increment(value) {
    var digits = normalize(value).split("")
    for (var i = digits.length - 1; i >= 0; --i) {
        if (digits[i] !== "9") {
            digits[i] = String(Number(digits[i]) + 1)
            return digits.join("")
        }
        digits[i] = "0"
    }
    digits.unshift("1")
    return digits.join("")
}

function pow10(exponent) {
    var value = "1"
    for (var i = 0; i < exponent; ++i)
        value += "0"
    return value
}

function parseHuman(text, decimals) {
    var value = String(text)
    if (value.length === 0)
        return { "ok": false, "code": "amount_required", "raw": "" }
    var match = /^(0|[1-9][0-9]*)(?:\.([0-9]*))?$/.exec(value)
    if (!match)
        return { "ok": false, "code": "invalid_amount_format", "raw": "" }

    var fraction = match[2] || ""
    if (fraction.length > decimals)
        return { "ok": false, "code": "invalid_amount_precision", "raw": "" }
    var raw = normalize(match[1] + fraction + pow10(decimals - fraction.length).slice(1))
    if (compare(raw, U128_MAX) > 0)
        return { "ok": false, "code": "invalid_raw_amount", "raw": "" }
    return { "ok": true, "code": "", "raw": raw }
}

function formatRaw(rawValue, decimals) {
    if (!isUnsigned(rawValue))
        return ""
    var raw = normalize(rawValue)
    if (decimals === 0)
        return raw
    while (raw.length <= decimals)
        raw = "0" + raw
    var whole = raw.slice(0, raw.length - decimals)
    var fraction = raw.slice(raw.length - decimals).replace(/0+$/, "")
    return fraction.length > 0 ? whole + "." + fraction : whole
}

function parsePrice(text) {
    var value = String(text)
    if (value.length === 0)
        return { "ok": false, "code": "amount_required" }
    var match = /^(0|[1-9][0-9]*)(?:\.([0-9]*))?$/.exec(value)
    if (!match)
        return { "ok": false, "code": "invalid_amount_format" }
    var fraction = match[2] || ""
    var numerator = normalize(match[1] + fraction)
    if (numerator === "0")
        return { "ok": false, "code": "amount_must_be_positive" }
    return {
        "ok": true,
        "code": "",
        "numerator": numerator,
        "scale": fraction.length
    }
}

function parseRatio(amountA, amountB) {
    var parsedA = parsePrice(amountA)
    if (!parsedA.ok)
        return { "ok": false, "code": parsedA.code }
    var parsedB = parsePrice(amountB)
    if (!parsedB.ok)
        return { "ok": false, "code": parsedB.code }

    return {
        "ok": true,
        "code": "",
        "numerator": multiply(parsedB.numerator, pow10(parsedA.scale)),
        "denominator": multiply(parsedA.numerator, pow10(parsedB.scale))
    }
}

function ratioToQ64(amountA, amountB, canonicalDecimalsA, canonicalDecimalsB,
                    displayIsCanonical) {
    var ratio = parseRatio(amountA, amountB)
    if (!ratio.ok)
        return { "ok": false, "code": ratio.code, "raw": "" }

    var numerator
    var denominator
    if (displayIsCanonical) {
        numerator = multiply(multiply(ratio.numerator, Q64), pow10(canonicalDecimalsB))
        denominator = multiply(ratio.denominator, pow10(canonicalDecimalsA))
    } else {
        numerator = multiply(multiply(ratio.denominator, Q64), pow10(canonicalDecimalsB))
        denominator = multiply(ratio.numerator, pow10(canonicalDecimalsA))
    }
    var raw = divide(numerator, denominator).quotient
    if (raw === "0")
        return { "ok": false, "code": "amount_too_low", "raw": "" }
    if (compare(raw, U128_MAX) > 0)
        return { "ok": false, "code": "invalid_raw_amount", "raw": "" }
    return { "ok": true, "code": "", "raw": raw }
}

function pairAmount(raw, fromA, decimalsA, decimalsB, priceAmountA, priceAmountB) {
    if (!isUnsigned(raw))
        return { "ok": false, "code": "invalid_raw_amount", "raw": "" }
    var ratio = parseRatio(priceAmountA, priceAmountB)
    if (!ratio.ok)
        return { "ok": false, "code": ratio.code, "raw": "" }

    var numerator
    var denominator
    if (fromA) {
        numerator = multiply(multiply(raw, ratio.numerator), pow10(decimalsB))
        denominator = multiply(ratio.denominator, pow10(decimalsA))
    } else {
        numerator = multiply(multiply(raw, ratio.denominator), pow10(decimalsA))
        denominator = multiply(ratio.numerator, pow10(decimalsB))
    }
    var result = divide(numerator, denominator)
    if (!result.valid)
        return { "ok": false, "code": "invalid_raw_amount", "raw": "" }
    var rounded = compare(multiply(result.remainder, "2"), denominator) >= 0
            ? increment(result.quotient) : result.quotient
    if (rounded === "0")
        return { "ok": false, "code": "amount_too_low", "raw": "" }
    if (compare(rounded, U128_MAX) > 0)
        return { "ok": false, "code": "invalid_raw_amount", "raw": "" }
    return { "ok": true, "code": "", "raw": rounded }
}

function ratioValue(amountA, amountB, precision) {
    var ratio = parseRatio(amountA, amountB)
    return ratio.ok ? formatRatio(ratio.numerator, ratio.denominator, precision) : ""
}

function formatRatio(numerator, denominator, precision) {
    var result = divide(numerator, denominator)
    if (!result.valid)
        return ""
    var fraction = ""
    var remainder = result.remainder
    for (var i = 0; i < precision && remainder !== "0"; ++i) {
        var digit = divide(multiply(remainder, "10"), denominator)
        fraction += digit.quotient
        remainder = digit.remainder
    }
    fraction = fraction.replace(/0+$/, "")
    return fraction.length > 0 ? result.quotient + "." + fraction : result.quotient
}

function priceFromQ64(rawValue, canonicalDecimalsA, canonicalDecimalsB, displayIsCanonical) {
    if (!isUnsigned(rawValue) || normalize(rawValue) === "0")
        return ""
    var numerator
    var denominator
    if (displayIsCanonical) {
        numerator = multiply(rawValue, pow10(canonicalDecimalsA))
        denominator = multiply(Q64, pow10(canonicalDecimalsB))
    } else {
        numerator = multiply(Q64, pow10(canonicalDecimalsB))
        denominator = multiply(rawValue, pow10(canonicalDecimalsA))
    }
    return formatRatio(numerator, denominator, 12)
}

function mulDivFloor(left, right, denominator) {
    return divide(multiply(left, right), denominator).quotient
}

function toU32(value) {
    if (!isUnsigned(value) || compare(value, U32_MAX) > 0)
        return -1
    return Number(normalize(value))
}
