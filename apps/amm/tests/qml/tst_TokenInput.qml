pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/components/liquidity" as Liquidity
import "../../qml/components/swap" as Swap

TestCase {
    id: testCase

    name: "TokenInput"
    readonly property string base58Id: "Eiw5zDP1BKukkxMY8dj7Hkw9NfCTh6iU5av5F7FT8ExC"
    readonly property string hexId: "cbe5e5fed00f9af47a0cbc6f96de828dd8e72090971fa1b904bec2014e3f634d"

    Component {
        id: inputComponent

        Swap.TokenInput {
            visible: false
            width: 400
            theme: inputTheme
            selectorObjectName: "accountSelector"

            Liquidity.AmmTheme {
                id: inputTheme
            }
        }
    }

    function createInput(definitionId) {
        return createTemporaryObject(inputComponent, testCase, {
            "token": { "definitionId": definitionId, "symbol": "TKA" },
            "holdings": [{
                "accountId": "holding-a",
                "accountType": "TokenHolding",
                "balanceRaw": "10",
                "definitionId": base58Id,
                "definitionIdHex": hexId
            }]
        })
    }

    function test_base58DefinitionSelectsHolding() {
        var input = createInput(base58Id)
        verify(input)
        var selector = findChild(input, "accountSelector")
        verify(selector)

        compare(selector.stateField, "definitionId")
        tryCompare(selector, "hasFunds", true)
        tryCompare(selector, "selectedAccountId", "holding-a")
    }

    function test_hexDefinitionSelectsHolding() {
        var input = createInput(hexId)
        verify(input)
        var selector = findChild(input, "accountSelector")
        verify(selector)

        compare(selector.stateField, "definitionIdHex")
        tryCompare(selector, "hasFunds", true)
        tryCompare(selector, "selectedAccountId", "holding-a")
    }
}
