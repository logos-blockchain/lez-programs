import QtQuick
import QtTest
import Logos.Wallet as Wallet

Item {
    id: root

    width: 480
    height: 320

    readonly property var holdingA: ({
        "accountId": "holding-a",
        "accountType": "TokenHolding",
        "definitionId": "token-a",
        "balanceRaw": "120"
    })
    readonly property var holdingB: ({
        "accountId": "holding-b",
        "accountType": "TokenHolding",
        "state": {
            "definitionId": "token-a",
            "balanceRaw": "80"
        }
    })
    readonly property var otherToken: ({
        "accountId": "holding-c",
        "accountType": "TokenHolding",
        "definitionId": "token-b",
        "balanceRaw": "50"
    })
    readonly property var otherType: ({
        "accountId": "pool-a",
        "accountType": "Pool",
        "definitionId": "token-a"
    })

    Component {
        id: selectorComponent

        Wallet.ProgramAccountSelector {
            width: 260
            accountType: "TokenHolding"
            stateField: "definitionId"
            stateValue: "token-a"
        }
    }

    TestCase {
        name: "ProgramAccountSelector"
        when: windowShown

        function test_inputNoFunds() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [root.otherToken, root.otherType],
                "selectionMode": Wallet.ProgramAccountSelector.Input
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "showEmptyInput", true)
            compare(selector.showCombo, false)
            compare(selector.hasFunds, false)
            compare(selector.ready, false)
            compare(selector.selectedAccountId, "")
        }

        function test_inputSingleHoldingAutoSelectsWithoutCombo() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [root.holdingA, root.otherToken],
                "selectionMode": Wallet.ProgramAccountSelector.Input
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "selectedAccountId", "holding-a")
            compare(selector.showCombo, false)
            compare(selector.hasFunds, true)
            compare(selector.ready, true)
            compare(selector.selectedBalanceRaw, "120")
        }

        function test_inputMultipleHoldingsSelectsHighestBalance() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [root.holdingA, root.holdingB],
                "selectionMode": Wallet.ProgramAccountSelector.Input
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "showCombo", true)
            compare(selector.matchingAccounts.length, 2)
            tryCompare(selector, "selectedAccountId", "holding-a")
            compare(selector.selectedBalanceRaw, "120")
            compare(selector.ready, true)

            selector.setSelection("holding-b", false)
            compare(selector.selectedAccountId, "holding-b")
            compare(selector.selectedBalanceRaw, "80")
            compare(selector.ready, true)
            selector.reconcileSelection()
            compare(selector.selectedAccountId, "holding-b")
        }

        function test_outputNoHoldingSelectsCreateNew() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [],
                "selectionMode": Wallet.ProgramAccountSelector.Output
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "createNewSelected", true)
            compare(selector.showCombo, true)
            compare(selector.choices.length, 1)
            compare(selector.ready, true)
        }

        function test_outputSingleHoldingOffersExistingAndCreateNew() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [root.holdingA],
                "selectionMode": Wallet.ProgramAccountSelector.Output
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "selectedAccountId", "holding-a")
            compare(selector.choices.length, 2)
            compare(selector.createNewSelected, false)
            compare(selector.ready, true)

            selector.setSelection("", true)
            compare(selector.selectedAccountId, "")
            compare(selector.createNewSelected, true)
            compare(selector.ready, true)
        }

        function test_outputMultipleHoldingsSelectsHighestAndOffersCreateNew() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [root.holdingA, root.holdingB],
                "selectionMode": Wallet.ProgramAccountSelector.Output
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "showCombo", true)
            compare(selector.choices.length, 3)
            tryCompare(selector, "selectedAccountId", "holding-a")
            compare(selector.createNewSelected, false)
            compare(selector.ready, true)

            selector.setSelection("", true)
            compare(selector.selectedAccountId, "")
            compare(selector.createNewSelected, true)
            compare(selector.ready, true)
        }

        function test_highestBalanceComparisonPreservesU128Precision() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [
                    {
                        "accountId": "holding-lower",
                        "accountType": "TokenHolding",
                        "definitionId": "token-a",
                        "balanceRaw": "900719925474099299999"
                    },
                    {
                        "accountId": "holding-higher",
                        "accountType": "TokenHolding",
                        "definitionId": "token-a",
                        "balanceRaw": "900719925474099300000"
                    }
                ],
                "selectionMode": Wallet.ProgramAccountSelector.Input
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "selectedAccountId", "holding-higher")
            compare(selector.matchingAccounts[0].accountId, "holding-higher")
        }

        function test_automaticCreateNewChangesToHoldingAfterWalletLoads() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [],
                "selectionMode": Wallet.ProgramAccountSelector.Output
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "createNewSelected", true)

            selector.sourceModel = [root.holdingB, root.holdingA]
            tryCompare(selector, "selectedAccountId", "holding-a")
            compare(selector.createNewSelected, false)
        }

        function test_matchesNumericZeroState() {
            const selector = createTemporaryObject(selectorComponent, root, {
                "sourceModel": [{
                    "accountId": "zero-state",
                    "accountType": "TokenHolding",
                    "state": { "version": 0 }
                }],
                "stateField": "version",
                "stateValue": 0,
                "selectionMode": Wallet.ProgramAccountSelector.Input
            })
            verify(!!selector, "Component exists")
            tryCompare(selector, "selectedAccountId", "zero-state")
            compare(selector.ready, true)
        }
    }
}
