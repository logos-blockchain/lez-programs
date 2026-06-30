import QtQuick 2.15

QtObject {
    id: root

    property string tokenA: ""
    property string tokenB: ""
    property string feeTier: "0%"
    property real userLpBalance: 0
    property real reserveA: 0
    property real reserveB: 0
    property real totalLpSupply: 0
    property real walletBalanceA: 0
    property real walletBalanceB: 0
    readonly property real minimumLiquidity: 1000

    readonly property real poolShare: totalLpSupply > 0 ? userLpBalance / totalLpSupply : 0
    readonly property real userOwnedA: reserveA * poolShare
    readonly property real userOwnedB: reserveB * poolShare
    readonly property real tokenAPerTokenB: reserveB > 0 ? Math.floor(reserveA / reserveB) : 0

    function loadConfig(config) {
        tokenA = config.tokenA || "";
        tokenB = config.tokenB || "";
        feeTier = config.feeTier || "0%";
        userLpBalance = Number(config.userLpBalance) || 0;
        reserveA = Number(config.reserveA) || 0;
        reserveB = Number(config.reserveB) || 0;
        totalLpSupply = Number(config.totalLpSupply) || 0;
        walletBalanceA = Number(config.walletBalanceA) || 0;
        walletBalanceB = Number(config.walletBalanceB) || 0;
    }

    function parseAmount(value) {
        return Math.max(0, Number(value) || 0);
    }

    function floorAmount(value) {
        return Math.floor(parseAmount(value));
    }

    function amountBForA(amountA) {
        if (reserveA <= 0) {
            return 0;
        }

        return Math.floor(reserveB * parseAmount(amountA) / reserveA);
    }

    function amountAForB(amountB) {
        if (reserveB <= 0) {
            return 0;
        }

        return Math.floor(reserveA * parseAmount(amountB) / reserveB);
    }

    function addLiquidityPreview(maxA, maxB) {
        const safeMaxA = floorAmount(maxA);
        const safeMaxB = floorAmount(maxB);
        const idealA = reserveB > 0 ? reserveA * safeMaxB / reserveB : 0;
        const idealB = reserveA > 0 ? reserveB * safeMaxA / reserveA : 0;
        const actualA = Math.floor(Math.min(idealA, safeMaxA));
        const actualB = Math.floor(Math.min(idealB, safeMaxB));
        const lpFromA = reserveA > 0 ? Math.floor(totalLpSupply * actualA / reserveA) : 0;
        const lpFromB = reserveB > 0 ? Math.floor(totalLpSupply * actualB / reserveB) : 0;

        return {
            "actualA": actualA,
            "actualB": actualB,
            "deltaLp": Math.min(lpFromA, lpFromB),
            "idealA": idealA,
            "idealB": idealB
        };
    }

    function maxAddLiquidityForBalances() {
        return addLiquidityPreview(walletBalanceA, walletBalanceB);
    }

    function clampBurnAmount(value) {
        return Math.min(floorAmount(value), floorAmount(userLpBalance));
    }

    function clampSlippageTolerancePercent(value) {
        return Math.max(0.01, Math.min(50, Number(value) || 0));
    }

    function minReceivedAmount(previewAmount, slippageTolerancePercent) {
        const safeAmount = floorAmount(previewAmount);
        const safeSlippage = clampSlippageTolerancePercent(slippageTolerancePercent);

        return Math.floor(safeAmount * (1 - safeSlippage / 100));
    }

    function burnAmountForPercent(percent) {
        const safePercent = Math.max(0, Math.min(100, Number(percent) || 0));

        return clampBurnAmount(Math.floor(userLpBalance * safePercent / 100));
    }

    function removeLiquidityPreview(burnedLp) {
        const safeBurnedLp = totalLpSupply > 0 ? Math.min(clampBurnAmount(burnedLp), floorAmount(totalLpSupply)) : 0;
        const withdrawA = totalLpSupply > 0 ? Math.floor(reserveA * safeBurnedLp / totalLpSupply) : 0;
        const withdrawB = totalLpSupply > 0 ? Math.floor(reserveB * safeBurnedLp / totalLpSupply) : 0;
        const newTotalLpSupply = Math.max(0, floorAmount(totalLpSupply) - safeBurnedLp);
        const newUserLpBalance = Math.max(0, floorAmount(userLpBalance) - safeBurnedLp);

        return {
            "burnedLp": safeBurnedLp,
            "newReserveA": Math.max(0, reserveA - withdrawA),
            "newReserveB": Math.max(0, reserveB - withdrawB),
            "newTotalLpSupply": newTotalLpSupply,
            "newUserLpBalance": newUserLpBalance,
            "newUserShare": newTotalLpSupply > 0 ? newUserLpBalance / newTotalLpSupply : 0,
            "withdrawA": withdrawA,
            "withdrawB": withdrawB
        };
    }

    function formatInteger(value) {
        const rounded = Math.round(Number(value) || 0);
        return rounded.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
    }

    function formatDecimal(value) {
        const amount = Number(value) || 0;

        if (Math.abs(amount - Math.round(amount)) < 0.000001) {
            return formatInteger(amount);
        }

        return amount.toFixed(6).replace(/0+$/, "").replace(/[.]$/, "");
    }

    function formatCompactDecimal(value) {
        const amount = Number(value) || 0;

        if (Math.abs(amount) >= 1000 || Math.abs(amount - Math.round(amount)) < 0.000001) {
            return formatInteger(amount);
        }

        if (Math.abs(amount) >= 1) {
            return amount.toFixed(2).replace(/0+$/, "").replace(/[.]$/, "");
        }

        return amount.toFixed(6).replace(/0+$/, "").replace(/[.]$/, "");
    }

    function formatInputAmount(value) {
        return formatDecimal(value);
    }

    function formatTokenAmount(value, token) {
        return formatDecimal(value) + " " + token;
    }

    function formatCompactTokenAmount(value, token) {
        return formatCompactDecimal(value) + " " + token;
    }

    function formatLpAmount(value) {
        return formatInteger(value) + " LP";
    }

    function formatPoolShare(value) {
        return "\u2248 " + (Math.max(0, Number(value) || 0) * 100).toFixed(2) + "%";
    }

    function formatPercent(value) {
        const amount = Math.max(0, Number(value) || 0);

        if (Math.abs(amount - Math.round(amount)) < 0.000001) {
            return Math.round(amount).toString() + "%";
        }

        return amount.toFixed(2).replace(/0+$/, "").replace(/[.]$/, "") + "%";
    }
}
