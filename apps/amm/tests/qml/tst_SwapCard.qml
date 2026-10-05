pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import Logos.Wallet
import "../../qml/components/swap" as Swap
import "../../qml/components/liquidity" as Liquidity

Item {
    id: scene
    width: 800
    height: 800

    Liquidity.AmmTheme { id: theme }

    Component {
        id: backendComponent
        QtObject {
            property bool isWalletOpen: true
            property int swapContextRevision: 1
            function resolvePoolAccount(a, b) { return { kind: "pool" } }
            function swapExactInQuote(a, b, amount, slippage) {
                return { kind: "in", slippage: slippage }
            }
            function swapExactOutQuote(a, b, amount, slippage) {
                return { kind: "out", slippage: slippage }
            }
            function confirmedSwap(intent) {
                return { kind: "submit", intent: JSON.parse(JSON.stringify(intent)) }
            }
        }
    }
    Component {
        id: runtimeComponent
        QtObject {
            property var requests: []
            function watch(request, success, failure) {
                requests.push({ request: request, success: success, failure: failure })
            }
            function latest(kind) {
                for (var i = requests.length - 1; i >= 0; --i) {
                    if (requests[i].request.kind === kind)
                        return requests[i]
                }
                return null
            }
            function submissions() {
                return requests.filter(function(entry) { return entry.request.kind === "submit" })
            }
        }
    }
    Component {
        id: cardComponent
        Swap.SwapCard {
            theme: theme
            width: 480
        }
    }
    Component {
        id: dialogComponent
        TransactionConfirmationDialog {
            property var card
            onConfirmed: function(snapshot) { card.executeSwap(snapshot) }
        }
    }

    TestCase {
        id: testCase
        name: "SwapCardConfirmation"
        when: windowShown
        readonly property string tokenA: "11111111111111111111111111111111"
        readonly property string tokenB: "22222222222222222222222222222222"
        readonly property string tokenC: "33333333333333333333333333333333"
        property var backend
        property var runtime

        function init() {
            backend = createTemporaryObject(backendComponent, scene)
            runtime = createTemporaryObject(runtimeComponent, scene)
            verify(backend)
            verify(runtime)
        }

        function holding(id, definition) {
            return { accountId: id, accountType: "TokenHolding", definitionId: definition,
                     balanceRaw: "340282366920938463463374607431768211455" }
        }

        function createCard(exactOut) {
            var card = createTemporaryObject(cardComponent, scene, {
                backend: backend,
                runtime: runtime,
                holdings: [holding("holdingA", tokenA), holding("holdingB", tokenB),
                           holding("holdingC", tokenC)]
            })
            verify(card)
            card.sellToken = { definitionId: tokenA, symbol: "SAME", name: "A" }
            card.buyToken = { definitionId: tokenB, symbol: "SAME", name: "B" }
            card.editingSide = exactOut ? "buy" : "sell"
            if (exactOut)
                card.buyInput = "1000"
            else
                card.sellInput = "1000"
            tryCompare(card, "sellHolding", "holdingA")
            tryCompare(card, "buyHolding", "holdingB")
            readyQuote(card)
            return card
        }

        function readyQuote(card) {
            card.doResolvePool()
            runtime.latest("pool").success({ status: "ok", reserveA: "1000000", reserveB: "1000000" })
            if (card.editingSide === "sell") {
                card.doQuoteIn()
                runtime.latest("in").success({ status: "ok", expectedOut: "1000", minReceived: "995" })
            } else {
                card.doQuoteOut()
                runtime.latest("out").success({ status: "ok", requiredIn: "1000", maxIn: "1005" })
            }
            compare(card.canSubmit, true)
        }

        function test_confirmedRequestPreservesExactAmounts_data() {
            return [{ tag: "exact-in", exactOut: false }, { tag: "exact-out", exactOut: true }]
        }

        function test_confirmedRequestPreservesExactAmounts(data) {
            var card = createCard(data.exactOut)
            var amount = "18446744073709551617"
            var bound = "18446744073709551599"
            if (data.exactOut) {
                card.buyInput = amount
                readyQuote(card)
                card.quoteMaxIn = bound
            } else {
                card.sellInput = amount
                readyQuote(card)
                card.quoteMinReceived = bound
            }
            var snapshot = card.buildSnapshot()
            var dialog = createTemporaryObject(dialogComponent, scene, { card: card })
            verify(dialog)
            dialog.openWithSnapshot(snapshot)
            dialog.confirm()
            compare(runtime.submissions().length, 1)
            var request = runtime.submissions()[0].request.intent
            compare(request.amount, amount)
            compare(request.bound, bound)
            compare(request.sellDefinitionId, tokenA)
            compare(request.buyDefinitionId, tokenB)
            compare(request.sellHoldingId, "holdingA")
            compare(request.buyHoldingId, "holdingB")
            compare(request.deadline, "18446744073709551615")
            compare(request.swapMode, data.exactOut ? "swap-exact-output" : "swap-exact-input")
            compare(request.contextRevision, 1)
        }

        function test_changedDetailsRequireNewConfirmation_data() {
            return ["amount", "bound", "mode", "definition", "holding", "context", "label"]
                .map(function(change) { return { tag: change, change: change } })
        }

        function test_changedDetailsRequireNewConfirmation(data) {
            var card = createCard(false)
            var snapshot = card.buildSnapshot()
            switch (data.change) {
            case "amount": card.sellInput = "2000"; readyQuote(card); break
            case "bound": card.quoteMinReceived = "950"; break
            case "mode": card.editingSide = "buy"; card.buyInput = "1000"; readyQuote(card); break
            case "definition":
                card.buyToken = { definitionId: tokenC, symbol: "SAME", name: "C" }
                tryCompare(card, "buyHolding", "holdingC")
                readyQuote(card)
                break
            case "holding":
                card.holdings = [holding("holdingA2", tokenA), holding("holdingB", tokenB)]
                tryCompare(card, "sellHolding", "holdingA2")
                break
            case "context": backend.swapContextRevision++; readyQuote(card); break
            case "label":
                card.buyToken = { definitionId: tokenB, symbol: "OTHER", name: "B" }
                readyQuote(card)
                break
            }
            card.executeSwap(snapshot)
            compare(runtime.submissions().length, 0)
            compare(card.swapError, "Swap details changed. Review the updated swap and confirm again.")
        }

        function test_oldSlippageQuoteCannotChangeConfirmation_data() {
            return [{ tag: "exact-in", exactOut: false }, { tag: "exact-out", exactOut: true }]
        }

        function test_oldSlippageQuoteCannotChangeConfirmation(data) {
            var card = createCard(data.exactOut)
            card.slippageTolerancePercent = 5
            if (data.exactOut) card.doQuoteOut(); else card.doQuoteIn()
            var earlier = runtime.latest(data.exactOut ? "out" : "in")
            card.slippageTolerancePercent = 0.5
            readyQuote(card)
            var snapshot = card.buildSnapshot()
            earlier.success(data.exactOut
                ? { status: "ok", requiredIn: "1000", maxIn: "1050" }
                : { status: "ok", expectedOut: "1000", minReceived: "950" })
            compare(card.bound, data.exactOut ? "1005" : "995")
            card.executeSwap(snapshot)
            compare(runtime.submissions().length, 1)
            compare(runtime.submissions()[0].request.intent.bound, data.exactOut ? "1005" : "995")
        }

        function test_oldContextQuoteCannotRestoreSubmit() {
            var card = createCard(false)
            card.doQuoteIn()
            var earlier = runtime.latest("in")
            backend.swapContextRevision++
            earlier.success({ status: "ok", expectedOut: "1000", minReceived: "995" })
            compare(card.quoteMinReceived, "0")
            compare(card.canSubmit, false)
        }

        function test_sourceContextRejectionShowsRecovery() {
            var card = createCard(false)
            card.executeSwap(card.buildSnapshot())
            runtime.latest("submit").success({ status: "error", error: "swap_context_changed" })
            compare(card.swapInProgress, false)
            compare(card.swapError, "Wallet or network changed. Review the updated swap and confirm again.")
        }
    }
}
