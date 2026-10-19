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

    function test_protocolActionsUseDisplayLabels() {
        var summary = createTemporaryObject(summaryComponent, testCase)
        verify(summary)

        summary.snapshot = { "poolExists": false }
        compare(summary.actionText(), "Create pool")
        summary.snapshot = { "poolExists": true }
        compare(summary.actionText(), "Add liquidity")
    }
}
