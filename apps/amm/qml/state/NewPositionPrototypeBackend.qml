import QtQuick 2.15

QtObject {
    id: root

    readonly property string activeAccount: "lz1q9s...42fd"
    readonly property var holdings: [
        {
            "symbol": "USDC",
            "name": "USD Coin",
            "definitionId": "token:usdc-testnet",
            "holdingId": "holding:active:usdc",
            "balance": 12450,
            "balanceText": "12,450.00 USDC",
            "accent": "#2E7CF6"
        },
        {
            "symbol": "LOGOS",
            "name": "Logos",
            "definitionId": "token:logos-testnet",
            "holdingId": "holding:active:logos",
            "balance": 850000,
            "balanceText": "850,000 LOGOS",
            "accent": "#F26A21"
        },
        {
            "symbol": "WETH",
            "name": "Wrapped Ether",
            "definitionId": "token:weth-testnet",
            "holdingId": "holding:active:weth",
            "balance": 3.25,
            "balanceText": "3.25 WETH",
            "accent": "#B7C2D8"
        }
    ]
    readonly property var feeTiers: [
        {
            "bps": 1,
            "label": "0.01%",
            "supported": true
        },
        {
            "bps": 5,
            "label": "0.05%",
            "supported": true
        },
        {
            "bps": 25,
            "label": "0.25%",
            "supported": false
        },
        {
            "bps": 30,
            "label": "0.30%",
            "supported": true
        },
        {
            "bps": 100,
            "label": "1.00%",
            "supported": true
        }
    ]
    readonly property real minimumLiquidity: 1000

    function holdingBySymbol(symbol) {
        for (let i = 0; i < root.holdings.length; ++i) {
            if (root.holdings[i].symbol === symbol)
                return root.holdings[i];
        }

        return null;
    }

    function feeTierByBps(bps) {
        for (let i = 0; i < root.feeTiers.length; ++i) {
            if (root.feeTiers[i].bps === bps)
                return root.feeTiers[i];
        }

        return null;
    }

    function feeLabel(bps) {
        const tier = feeTierByBps(bps);
        return tier ? tier.label : qsTr("%1 bps").arg(bps);
    }

    function parseAmount(value) {
        const parsed = Number(value);
        return isFinite(parsed) && parsed > 0 ? parsed : 0;
    }

    function formatAmount(value) {
        const amount = Math.max(0, Number(value) || 0);

        if (amount >= 1000)
            return amount.toFixed(2).replace(/\.00$/, "").replace(/\B(?=(\d{3})+(?!\d))/g, ",");

        if (amount >= 1)
            return amount.toFixed(4).replace(/0+$/, "").replace(/[.]$/, "");

        return amount.toFixed(6).replace(/0+$/, "").replace(/[.]$/, "");
    }

    function formatTokenAmount(value, symbol) {
        return qsTr("%1 %2").arg(formatAmount(value)).arg(symbol);
    }

    function unorderedPairKey(symbolA, symbolB) {
        return symbolA < symbolB ? symbolA + "/" + symbolB : symbolB + "/" + symbolA;
    }

    function poolContext(symbolA, symbolB) {
        if (!symbolA || !symbolB || symbolA === symbolB) {
            return {
                "poolStatus": "unavailable_pool",
                "statusLabel": qsTr("Unavailable"),
                "detail": qsTr("Choose two different assets from the active account."),
                "instruction": "",
                "storedFeeBps": 0,
                "poolId": "",
                "priceText": "",
                "reserveText": ""
            };
        }

        const key = unorderedPairKey(symbolA, symbolB);

        if (key === "LOGOS/USDC") {
            return {
                "poolStatus": "active_pool",
                "statusLabel": qsTr("Active pool"),
                "detail": qsTr("Deposits are quoted against the existing pool ratio. Nonmatching fee tiers are locked."),
                "instruction": "add_liquidity",
                "storedFeeBps": 30,
                "poolId": "pool:usdc-logos",
                "priceText": qsTr("1 USDC = 8 LOGOS"),
                "reserveText": qsTr("1,250,000 USDC / 10,000,000 LOGOS")
            };
        }

        if (key === "USDC/WETH") {
            return {
                "poolStatus": "missing_pool",
                "statusLabel": qsTr("Missing pool"),
                "detail": qsTr("Set the initial price first, then scale both deposits together."),
                "instruction": "new_definition",
                "storedFeeBps": 0,
                "poolId": "pool:usdc-weth",
                "priceText": qsTr("No reserves yet"),
                "reserveText": qsTr("Pool account is empty")
            };
        }

        return {
            "poolStatus": "unavailable_pool",
            "statusLabel": qsTr("Unavailable"),
            "detail": qsTr("A pool account exists, but it cannot be quoted safely for this prototype state."),
            "instruction": "",
            "storedFeeBps": 0,
            "poolId": "pool:logos-weth",
            "priceText": qsTr("Quote disabled"),
            "reserveText": qsTr("Unsupported stored pool state")
        };
    }

    function activeQuote(request, context) {
        const tokenA = holdingBySymbol(request.tokenA);
        const tokenB = holdingBySymbol(request.tokenB);
        const inputA = parseAmount(request.amountA);
        const inputB = parseAmount(request.amountB);
        const editA = request.editedSide !== "B";
        const amountA = editA ? inputA : inputB / activeRatio(request.tokenA, request.tokenB);
        const amountB = editA ? inputA * activeRatio(request.tokenA, request.tokenB) : inputB;
        const expectedLp = Math.floor(Math.min(amountA * 5.5, amountB * 0.69));
        const minLp = Math.floor(expectedLp * (10000 - request.slippageBps) / 10000);

        if (inputA <= 0 && inputB <= 0)
            return quoteError(context, request, qsTr("Enter a deposit amount to preview LP output."));

        if (amountA > tokenA.balance)
            return quoteError(context, request, qsTr("Insufficient %1 balance.").arg(tokenA.symbol));

        if (amountB > tokenB.balance)
            return quoteError(context, request, qsTr("Insufficient %1 balance.").arg(tokenB.symbol));

        if (minLp <= 0)
            return quoteError(context, request, qsTr("LP minimum rounds to zero. Increase deposit amount."));

        return quoteOk(context, request, {
            "maxA": amountA,
            "maxB": amountB,
            "actualA": amountA,
            "actualB": amountB,
            "expectedLp": expectedLp,
            "minimumLp": minLp,
            "lockedLp": 0,
            "position": {
                "userLp": "148,320 LP",
                "share": "1.18%",
                "ownedA": request.tokenA === "USDC" ? "14,750 USDC" : "118,000 LOGOS",
                "ownedB": request.tokenB === "LOGOS" ? "118,000 LOGOS" : "14,750 USDC"
            },
            "accountChanges": [
                {
                    "role": qsTr("Config"),
                    "id": "amm:config",
                    "action": qsTr("Read")
                },
                {
                    "role": qsTr("Pool"),
                    "id": context.poolId,
                    "action": qsTr("Update")
                },
                {
                    "role": qsTr("Vault A"),
                    "id": "vault:" + request.tokenA.toLowerCase(),
                    "action": qsTr("Update")
                },
                {
                    "role": qsTr("Vault B"),
                    "id": "vault:" + request.tokenB.toLowerCase(),
                    "action": qsTr("Update")
                },
                {
                    "role": qsTr("User LP holding"),
                    "id": "holding:active:lp",
                    "action": qsTr("Update or create")
                },
                {
                    "role": qsTr("Clock"),
                    "id": "clock:canonical",
                    "action": qsTr("Read")
                }
            ]
        });
    }

    function missingQuote(request, context) {
        const tokenA = holdingBySymbol(request.tokenA);
        const tokenB = holdingBySymbol(request.tokenB);
        const price = parseAmount(request.initialPrice) || defaultInitialPrice(request.tokenA, request.tokenB);
        const scale = Math.max(1, Number(request.depositScale) || 1);
        const amountA = price >= 1 ? price * scale : scale;
        const amountB = price >= 1 ? scale : scale / price;
        const expectedLp = Math.floor(Math.sqrt(amountA * amountB) * 48);
        const userLp = Math.max(0, expectedLp - root.minimumLiquidity);
        const minLp = Math.floor(userLp * (10000 - request.slippageBps) / 10000);

        if (amountA > tokenA.balance)
            return quoteError(context, request, qsTr("Initial deposit exceeds %1 balance.").arg(tokenA.symbol));

        if (amountB > tokenB.balance)
            return quoteError(context, request, qsTr("Initial deposit exceeds %1 balance.").arg(tokenB.symbol));

        if (userLp <= 0)
            return quoteError(context, request, qsTr("Deposit must mint more than the locked minimum liquidity."));

        return quoteOk(context, request, {
            "maxA": amountA,
            "maxB": amountB,
            "actualA": amountA,
            "actualB": amountB,
            "expectedLp": userLp,
            "minimumLp": minLp,
            "lockedLp": root.minimumLiquidity,
            "position": {
                "userLp": "0 LP",
                "share": "New pool",
                "ownedA": "0 " + request.tokenA,
                "ownedB": "0 " + request.tokenB
            },
            "accountChanges": [
                {
                    "role": qsTr("Config"),
                    "id": "amm:config",
                    "action": qsTr("Read")
                },
                {
                    "role": qsTr("Pool"),
                    "id": context.poolId,
                    "action": qsTr("Create")
                },
                {
                    "role": qsTr("Vault A"),
                    "id": "vault:" + request.tokenA.toLowerCase(),
                    "action": qsTr("Update or create")
                },
                {
                    "role": qsTr("Vault B"),
                    "id": "vault:" + request.tokenB.toLowerCase(),
                    "action": qsTr("Update or create")
                },
                {
                    "role": qsTr("LP definition"),
                    "id": "lp:" + context.poolId,
                    "action": qsTr("Create")
                },
                {
                    "role": qsTr("LP lock holding"),
                    "id": "holding:lp-lock",
                    "action": qsTr("Create")
                },
                {
                    "role": qsTr("User LP holding"),
                    "id": "holding:active:lp",
                    "action": qsTr("Update or create")
                },
                {
                    "role": qsTr("Current tick"),
                    "id": "twap:" + context.poolId,
                    "action": qsTr("Create")
                },
                {
                    "role": qsTr("Clock"),
                    "id": "clock:canonical",
                    "action": qsTr("Read")
                }
            ]
        });
    }

    function quoteNewPosition(request) {
        const context = poolContext(request.tokenA, request.tokenB);
        const tier = feeTierByBps(request.feeBps);

        if (!tier || !tier.supported)
            return quoteError(context, request, qsTr("Fee tier is not supported by the AMM program."));

        if (context.poolStatus === "active_pool" && request.feeBps !== context.storedFeeBps)
            return quoteError(context, request, qsTr("Existing pool uses %1.").arg(feeLabel(context.storedFeeBps)));

        if (context.poolStatus === "active_pool")
            return activeQuote(request, context);

        if (context.poolStatus === "missing_pool")
            return missingQuote(request, context);

        return quoteError(context, request, context.detail);
    }

    function submitNewPosition(request, quoteHash) {
        const quote = quoteNewPosition(request);

        if (quote.status !== "ok") {
            return {
                "status": "error",
                "error": quote.error
            };
        }

        if (quote.quoteHash !== quoteHash) {
            return {
                "status": "error",
                "error": qsTr("Quote changed. Refresh preview before submitting.")
            };
        }

        return {
            "status": "ok",
            "message": quote.instruction === "new_definition" ? qsTr("Pool creation submitted") : qsTr("Liquidity deposit submitted"),
            "detail": qsTr("%1 / %2").arg(request.tokenA).arg(request.tokenB)
        };
    }

    function quoteOk(context, request, amounts) {
        return {
            "status": "ok",
            "error": "",
            "poolStatus": context.poolStatus,
            "statusLabel": context.statusLabel,
            "statusDetail": context.detail,
            "instruction": context.instruction,
            "feeBps": request.feeBps,
            "feeLabel": feeLabel(request.feeBps),
            "quoteHash": quoteHash(request),
            "pool": {
                "id": context.poolId,
                "priceText": context.poolStatus === "missing_pool" ? qsTr("1 %1 = %2 %3").arg(request.tokenB).arg(formatAmount(parseAmount(request.initialPrice) || defaultInitialPrice(request.tokenA, request.tokenB))).arg(request.tokenA) : context.priceText,
                "reserveText": context.reserveText
            },
            "deposit": {
                "maxA": amountValue(amounts.maxA, request.tokenA),
                "maxB": amountValue(amounts.maxB, request.tokenB),
                "actualA": amountValue(amounts.actualA, request.tokenA),
                "actualB": amountValue(amounts.actualB, request.tokenB)
            },
            "lp": {
                "expected": amountValue(amounts.expectedLp, "LP"),
                "minimum": amountValue(amounts.minimumLp, "LP"),
                "locked": amountValue(amounts.lockedLp, "LP")
            },
            "position": amounts.position,
            "accountChanges": amounts.accountChanges
        };
    }

    function quoteError(context, request, errorText) {
        return {
            "status": "error",
            "error": errorText,
            "poolStatus": context.poolStatus,
            "statusLabel": context.statusLabel,
            "statusDetail": context.detail,
            "instruction": context.instruction,
            "feeBps": request.feeBps,
            "feeLabel": feeLabel(request.feeBps),
            "quoteHash": quoteHash(request),
            "pool": {
                "id": context.poolId,
                "priceText": context.priceText,
                "reserveText": context.reserveText
            },
            "deposit": {
                "maxA": amountValue(0, request.tokenA),
                "maxB": amountValue(0, request.tokenB),
                "actualA": amountValue(0, request.tokenA),
                "actualB": amountValue(0, request.tokenB)
            },
            "lp": {
                "expected": amountValue(0, "LP"),
                "minimum": amountValue(0, "LP"),
                "locked": amountValue(context.poolStatus === "missing_pool" ? root.minimumLiquidity : 0, "LP")
            },
            "position": {
                "userLp": "0 LP",
                "share": "-",
                "ownedA": "0 " + request.tokenA,
                "ownedB": "0 " + request.tokenB
            },
            "accountChanges": []
        };
    }

    function amountValue(value, symbol) {
        const amount = Math.max(0, Number(value) || 0);
        return {
            "value": amount,
            "input": formatAmount(amount),
            "display": formatTokenAmount(amount, symbol),
            "symbol": symbol
        };
    }

    function activeRatio(symbolA, symbolB) {
        if (symbolA === "USDC" && symbolB === "LOGOS")
            return 8;

        if (symbolA === "LOGOS" && symbolB === "USDC")
            return 0.125;

        return 1;
    }

    function defaultInitialPrice(symbolA, symbolB) {
        if (symbolA === "USDC" && symbolB === "WETH")
            return 2500;

        if (symbolA === "WETH" && symbolB === "USDC")
            return 0.0004;

        return 1;
    }

    function quoteHash(request) {
        return "demo-" + request.tokenA + "-" + request.tokenB + "-" + request.feeBps + "-" + request.editedSide + "-" + request.amountA + "-" + request.amountB + "-" + request.initialPrice + "-" + request.depositScale + "-" + request.slippageBps;
    }
}
