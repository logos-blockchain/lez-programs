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

    readonly property real poolShare: totalLpSupply > 0 ? userLpBalance / totalLpSupply : 0
    readonly property real userOwnedA: reserveA * poolShare
    readonly property real userOwnedB: reserveB * poolShare

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
    }

    function formatInteger(value) {
        const rounded = Math.round(Number(value) || 0);
        return rounded.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
    }

    function formatTokenAmount(value, token) {
        return formatInteger(value) + " " + token;
    }

    function formatLpAmount(value) {
        return formatInteger(value) + " LP";
    }

    function formatPoolShare(value) {
        return "\u2248 " + (Math.max(0, Number(value) || 0) * 100).toFixed(2) + "%";
    }
}
