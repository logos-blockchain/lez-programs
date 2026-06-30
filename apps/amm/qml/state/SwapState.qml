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
