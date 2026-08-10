import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root

    property var snapshot: ({})

    spacing: 8

    function actionText() {
        return root.snapshot.poolExists ? qsTr("Add liquidity") : qsTr("Create pool")
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Pair")
        value: root.snapshot.pairText || "-"
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Action")
        value: root.actionText()
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
