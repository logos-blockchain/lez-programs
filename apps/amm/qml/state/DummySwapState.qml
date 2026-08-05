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
