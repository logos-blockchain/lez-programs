import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root

    property var snapshot: ({})

    spacing: 8

    function actionText(instruction) {
        if (instruction === "NewDefinition")
            return qsTr("Create pool")
        if (instruction === "AddLiquidity")
            return qsTr("Add liquidity")
        return instruction || "-"
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Pair")
        value: root.snapshot.pairText || "-"
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Action")
        value: root.actionText(root.snapshot.instruction)
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Fee")
        value: root.snapshot.feeText || "-"
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Deposit")
        value: qsTr("%1 + %2")
            .arg(root.snapshot.depositAText || "-")
            .arg(root.snapshot.depositBText || "-")
        valueWrapAnywhere: true
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Expected LP")
        value: root.snapshot.expectedLpText || "-"
    }
}
