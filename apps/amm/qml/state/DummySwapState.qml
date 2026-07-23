import QtQuick 2.15

QtObject {
    id: root

    property int feeBps: 30

    function parseAmount(value) {
        return Math.max(0, Number(value) || 0);
    }

    function clampSlippagePercent(value) {
        return Math.max(0, Math.min(50, Number(value) || 0));
    }

    function feeAmount(amountIn) {
        return parseAmount(amountIn) * root.feeBps / 10000;
    }

    function amountOutFor(amountIn, reserveIn, reserveOut) {
        const safeAmountIn = parseAmount(amountIn);
        const safeReserveIn = parseAmount(reserveIn);
        const safeReserveOut = parseAmount(reserveOut);

        if (safeAmountIn <= 0 || safeReserveIn <= 0 || safeReserveOut <= 0) {
            return 0;
        }

        const amountInAfterFee = safeAmountIn * (10000 - root.feeBps) / 10000;
        return safeReserveOut * amountInAfterFee / (safeReserveIn + amountInAfterFee);
    }

    function amountInFor(amountOut, reserveIn, reserveOut) {
        const safeAmountOut = parseAmount(amountOut);
        const safeReserveIn = parseAmount(reserveIn);
        const safeReserveOut = parseAmount(reserveOut);

        if (safeAmountOut <= 0 || safeReserveIn <= 0 || safeReserveOut <= 0) {
            return 0;
        }
        if (safeAmountOut >= safeReserveOut) {
            return 0;
        }

        const amountInAfterFee = safeAmountOut * safeReserveIn / (safeReserveOut - safeAmountOut);
        return amountInAfterFee * 10000 / (10000 - root.feeBps);
    }

    function priceImpactPercent(amountIn, amountOut, reserveIn, reserveOut) {
        const safeAmountIn = parseAmount(amountIn);
        const safeAmountOut = parseAmount(amountOut);
        const safeReserveIn = parseAmount(reserveIn);
        const safeReserveOut = parseAmount(reserveOut);

        if (safeAmountIn <= 0 || safeAmountOut <= 0) {
            return 0;
        }
        if (safeReserveIn <= 0 || safeReserveOut <= 0) {
            return 0;
        }
        if (safeReserveOut - safeAmountOut <= 0) {
            return 0;
        }

        const priceBefore = safeReserveIn / safeReserveOut;
        const priceAfter = (safeReserveIn + safeAmountIn) / (safeReserveOut - safeAmountOut);
        return (priceAfter - priceBefore) / priceBefore * 100;
    }

    function minReceived(amountOut, slippagePercent) {
        const safeAmount = parseAmount(amountOut);
        const safeSlippage = clampSlippagePercent(slippagePercent);
        return safeAmount * (1 - safeSlippage / 100);
    }

    // Exact-integer minimum-received (base units) for a SwapExactInput, used as
    // the on-chain slippage floor that is actually submitted. Computed in BigInt
    // (arbitrary precision, mirroring the on-chain u256 math): base units for
    // 18-decimal tokens exceed 2^53 and even overflow u128 intermediates, so JS
    // doubles silently lose precision and would understate min_out — weakening
    // the user's price protection. amountIn/reserveIn/reserveOut are base-unit
    // integer strings; returns a decimal string. Falls back to the double
    // estimate only if BigInt is unavailable in this Qt build.
    function minOutBaseUnits(amountIn, reserveIn, reserveOut, slippagePercent) {
        if (typeof BigInt !== "undefined") {
            var toBig = function (x) {
                var s = String(x).trim();
                return /^[0-9]+$/.test(s) ? BigInt(s) : BigInt(0);
            };
            var zero = BigInt(0);
            var denom = BigInt(10000);
            var amtIn = toBig(amountIn);
            var resIn = toBig(reserveIn);
            var resOut = toBig(reserveOut);
            if (amtIn <= zero || resIn <= zero || resOut <= zero)
                return "0";

            var feeBps = BigInt(Math.round(Math.min(10000, Math.max(0, Number(root.feeBps) || 0))));
            var amtInAfterFee = amtIn * (denom - feeBps) / denom;      // floor
            if (amtInAfterFee <= zero)
                return "0";
            var out = resOut * amtInAfterFee / (resIn + amtInAfterFee); // floor
            var slipBps = BigInt(Math.round(clampSlippagePercent(slippagePercent) * 100));
            if (slipBps < zero) slipBps = zero;
            if (slipBps > denom) slipBps = denom;
            var minOut = out * (denom - slipBps) / denom;              // floor
            return minOut.toString();
        }
        // Legacy double fallback (no worse than before if BigInt is missing).
        var estOut = amountOutFor(amountIn, reserveIn, reserveOut);
        return String(Math.floor(Math.max(0, minReceived(estOut, slippagePercent))));
    }

    function maxSent(amountIn, slippagePercent) {
        const safeAmount = parseAmount(amountIn);
        const safeSlippage = clampSlippagePercent(slippagePercent);
        return safeAmount * (1 + safeSlippage / 100);
    }

    function formatAmountValue(value) {
        const amount = Math.max(0, Number(value) || 0);
        if (amount >= 1) return amount.toFixed(2);
        if (amount >= 0.0001) return amount.toFixed(6);
        return amount.toFixed(8);
    }

    function formatTokenAmount(value, symbol) {
        const formatted = formatAmountValue(value);
        return symbol ? formatted + " " + symbol : formatted;
    }

    function formatPercent(value) {
        const amount = Number(value) || 0;
        if (amount > 0 && amount < 0.01) return "<0.01%";
        return amount.toFixed(2) + "%";
    }

    function formatSlippagePercent(value) {
        const amount = clampSlippagePercent(value);
        return amount.toFixed(2).replace(/0+$/, "").replace(/[.]$/, "") + "%";
    }
}
