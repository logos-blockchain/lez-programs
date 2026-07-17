pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/components/liquidity" as Liquidity

TestCase {
    id: testCase

    name: "LiquidityConfirmationSummary"

    Component {
        id: summaryComponent

        Liquidity.LiquidityConfirmationSummary {
            width: 480
        }
    }

    SignalSpy {
        id: editedSpy
        signalName: "snapshotEdited"
    }

    function test_protocolActionsUseDisplayLabels() {
        var summary = createTemporaryObject(summaryComponent, testCase)
        verify(summary)

        compare(summary.actionText("NewDefinition"), "Create pool")
        compare(summary.actionText("AddLiquidity"), "Add liquidity")
        compare(summary.actionText(""), "-")
    }

    function test_lpDestinationOffersExistingHoldingsAndCreateNew() {
        var summary = createTemporaryObject(summaryComponent, testCase, {
            "snapshot": {
                "instruction": "AddLiquidity",
                "request": ({ "schema": "new-position.v2" }),
                "lpHoldingOptions": [{
                    "holdingId": "44444444444444444444444444444444",
                    "balanceRaw": "7"
                }],
                "lpDestinationRequired": true,
                "quoteReady": false
            }
        })
        verify(summary)
        editedSpy.target = summary
        editedSpy.clear()

        var rows = summary.destinationRows()
        compare(rows.length, 2)
        compare(rows[1].createFresh, true)
        summary.selectDestination(rows[0])
        compare(editedSpy.count, 1)
        compare(editedSpy.signalArguments[0][0].request.lpHoldingId,
                "44444444444444444444444444444444")
        compare(editedSpy.signalArguments[0][0].quoteReady, false)
        editedSpy.target = null
    }
}
