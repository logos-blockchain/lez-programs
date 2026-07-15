import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root

    property var snapshot: ({})
    readonly property bool isAdd: root.snapshot.action === "add"

    spacing: 8

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 8
        visible: root.isAdd

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Deposit %1").arg(root.snapshot.tokenA || "")
            value: root.snapshot.depositA || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Deposit %1").arg(root.snapshot.tokenB || "")
            value: root.snapshot.depositB || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Receive at least")
            value: root.snapshot.minLpReceived || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Current ratio")
            value: root.snapshot.currentRatio || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Fee tier")
            value: root.snapshot.feeTier || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Slippage tolerance")
            value: root.snapshot.slippageTolerance || ""
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 8
        visible: !root.isAdd

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Burn LP")
            value: qsTr("%1 (%2)")
                .arg(root.snapshot.burnText || "")
                .arg(root.snapshot.burnPercent || "")
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Receive at least %1").arg(root.snapshot.tokenA || "")
            value: root.snapshot.minTokenAReceived || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Receive at least %1").arg(root.snapshot.tokenB || "")
            value: root.snapshot.minTokenBReceived || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Slippage tolerance")
            value: root.snapshot.slippageTolerance || ""
        }

        SummaryRow {
            Layout.fillWidth: true
            label: qsTr("Post-removal share")
            value: root.snapshot.postRemovalShare || ""
        }
    }
}
