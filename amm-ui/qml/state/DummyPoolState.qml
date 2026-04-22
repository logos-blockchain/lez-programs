import QtQuick 2.15

QtObject {
    id: root

    property string tokenA: "USDC"
    property string tokenB: "ETH"
    property string feeTier: "0.30%"
    property real userLpBalance: 1118033
    property real reserveA: 1000000
    property real reserveB: 500
    property real totalLpSupply: 22360679
    property real walletBalanceA: 60000
    property real walletBalanceB: 20

    readonly property real poolShare: totalLpSupply > 0 ? userLpBalance / totalLpSupply : 0
    readonly property real userOwnedA: reserveA * poolShare
    readonly property real userOwnedB: reserveB * poolShare
    readonly property real tokenAPerTokenB: reserveB > 0 ? Math.floor(reserveA / reserveB) : 0

    function applyAddLiquidity(actualA, actualB, mintedLp) {
        const safeA = Math.max(0, Number(actualA) || 0);
        const safeB = Math.max(0, Number(actualB) || 0);
        const safeLp = Math.max(0, Number(mintedLp) || 0);

        reserveA += safeA;
        reserveB += safeB;
        totalLpSupply += safeLp;
        userLpBalance += safeLp;
    }

    function applyRemoveLiquidity(withdrawA, withdrawB, burnedLp) {
        const safeA = Math.max(0, Number(withdrawA) || 0);
        const safeB = Math.max(0, Number(withdrawB) || 0);
        const safeLp = Math.max(0, Number(burnedLp) || 0);

        reserveA = Math.max(0, reserveA - safeA);
        reserveB = Math.max(0, reserveB - safeB);
        totalLpSupply = Math.max(0, totalLpSupply - safeLp);
        userLpBalance = Math.max(0, userLpBalance - safeLp);
    }

    function resetDummyState() {
        tokenA = "USDC";
        tokenB = "ETH";
        feeTier = "0.30%";
        userLpBalance = 1118033;
        reserveA = 1000000;
        reserveB = 500;
        totalLpSupply = 22360679;
        walletBalanceA = 60000;
        walletBalanceB = 20;
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

        return reserveB * parseAmount(amountA) / reserveA;
    }

    function amountAForB(amountB) {
        if (reserveB <= 0) {
            return 0;
        }

        return reserveA * parseAmount(amountB) / reserveB;
    }

    function addLiquidityPreview(maxA, maxB) {
        const safeMaxA = parseAmount(maxA);
        const safeMaxB = parseAmount(maxB);
        const idealA = reserveB > 0 ? reserveA * safeMaxB / reserveB : 0;
        const idealB = reserveA > 0 ? reserveB * safeMaxA / reserveA : 0;
        const actualA = Math.min(idealA, safeMaxA);
        const actualB = Math.min(idealB, safeMaxB);
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

    function formatInputAmount(value) {
        return formatDecimal(value);
    }

    function formatTokenAmount(value, token) {
        return formatDecimal(value) + " " + token;
    }

    function formatLpAmount(value) {
        return formatInteger(value) + " LP";
    }

    function formatPoolShare(value) {
        return "\u2248 " + (Math.max(0, Number(value) || 0) * 100).toFixed(2) + "%";
    }
}
