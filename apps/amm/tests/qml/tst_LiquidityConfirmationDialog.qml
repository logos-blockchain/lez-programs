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

        compare(summary.actionText("NewDefinition"), "Create pool")
        compare(summary.actionText("AddLiquidity"), "Add liquidity")
        compare(summary.actionText(""), "-")
    }
}
