pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../../qml/components/swap" as Swap
import "../../qml/components/liquidity" as Liquidity

Item {
    id: scene
    width: 800
    height: 800

    Liquidity.AmmTheme { id: theme }

    Component {
        id: summaryComponent
        Swap.SwapConfirmationSummary {
            theme: theme
            width: 280
        }
    }

    TestCase {
        id: testCase
        name: "SwapConfirmationIdentity"
        when: windowShown
        readonly property string definitionA: "1111111111111111111111111111111111111111111111111111111111111111"
        readonly property string definitionB: "2222222222222222222222222222222222222222222222222222222222222222"

        function snapshot(symbol) {
            return {
                intent: { sellDefinitionId: definitionA, buyDefinitionId: definitionB,
                          sellHoldingId: definitionB, buyHoldingId: definitionA },
                sellToken: symbol, buyToken: symbol,
                sellAmount: "1000", buyAmount: "1000", boundValue: "995",
                swapMode: "swap-exact-input", feeAmount: "3 " + symbol
            }
        }

        function test_fullDefinitionsRemainVisibleWithDuplicateOrEmptySymbols_data() {
            return [{ tag: "same-symbol", symbol: "USDC" }, { tag: "empty-symbol", symbol: "" }]
        }

        function test_fullDefinitionsRemainVisibleWithDuplicateOrEmptySymbols(data) {
            var summary = createTemporaryObject(summaryComponent, scene, { snapshot: snapshot(data.symbol) })
            verify(summary)
            var sell = findChild(summary, "swapConfirmedSellDefinition")
            var buy = findChild(summary, "swapConfirmedBuyDefinition")
            var pay = findChild(summary, "swapConfirmedPay")
            verify(sell)
            verify(buy)
            verify(pay)
            compare(sell.text, "Token definition: " + definitionA)
            compare(buy.text, "Token definition: " + definitionB)
            compare(sell.textFormat, Text.PlainText)
            compare(buy.textFormat, Text.PlainText)
            compare(sell.elide, Text.ElideNone)
            compare(buy.elide, Text.ElideNone)
            compare(sell.wrapMode, Text.WrapAnywhere)
            compare(buy.wrapMode, Text.WrapAnywhere)
            compare(pay.text, "1000 " + (data.symbol || "Token"))
        }

        function test_registryMarkupIsLiteralEverywhere() {
            var markup = "<b>USDC</b>"
            var summary = createTemporaryObject(summaryComponent, scene, { snapshot: snapshot(markup) })
            verify(summary)
            var expected = [
                ["swapConfirmedPay", "1000 " + markup],
                ["swapConfirmedReceive", "995 " + markup],
                ["swapSummaryFee", "3 " + markup],
                ["swapSummaryBound", "995 " + markup]
            ]
            for (var i = 0; i < expected.length; ++i) {
                var field = findChild(summary, expected[i][0])
                verify(field)
                compare(field.text, expected[i][1])
                compare(field.textFormat, Text.PlainText)
            }
        }

        function test_exactOutputShowsCapturedMaximumAndAccounts() {
            var value = snapshot("USDC")
            value.swapMode = "swap-exact-output"
            value.boundValue = "1005"
            var summary = createTemporaryObject(summaryComponent, scene, { snapshot: value })
            verify(summary)
            var pay = findChild(summary, "swapConfirmedPay")
            var receive = findChild(summary, "swapConfirmedReceive")
            var source = findChild(summary, "swapConfirmedSellHolding")
            var destination = findChild(summary, "swapConfirmedBuyHolding")
            verify(pay)
            verify(receive)
            verify(source)
            verify(destination)
            compare(pay.text, "1005 USDC")
            compare(receive.text, "1000 USDC")
            compare(source.text, "From account: " + definitionB)
            compare(destination.text, "To account: " + definitionA)
        }
    }
}
