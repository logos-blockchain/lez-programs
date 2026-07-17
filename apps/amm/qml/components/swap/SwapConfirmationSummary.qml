import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root

    property var theme
    property var snapshot: ({})

    spacing: 10

    Rectangle {
        Layout.fillWidth: true
        color: root.theme.colors.inputBg
        radius: 8
        implicitHeight: payColumn.implicitHeight + 24

        ColumnLayout {
            id: payColumn
            anchors.fill: parent
            anchors.margins: 12
            spacing: 4

            Text {
                objectName: "swapPayLabel"
                Layout.fillWidth: true
                text: root.snapshot.swapMode === "swap-exact-output"
                    ? qsTr("You pay at most")
                    : qsTr("You pay")
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
            }

            Text {
                objectName: "swapPayAmount"
                Layout.fillWidth: true
                text: qsTr("%1 %2")
                    .arg(root.snapshot.swapMode === "swap-exact-output"
                        ? root.snapshot.maxSent || ""
                        : root.snapshot.sellAmount || "")
                    .arg(root.snapshot.sellToken || "")
                color: root.theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 18
                elide: Text.ElideRight
            }
        }
    }

    Rectangle {
        Layout.fillWidth: true
        color: root.theme.colors.inputBg
        radius: 8
        implicitHeight: receiveColumn.implicitHeight + 24

        ColumnLayout {
            id: receiveColumn
            anchors.fill: parent
            anchors.margins: 12
            spacing: 4

            Text {
                objectName: "swapReceiveLabel"
                Layout.fillWidth: true
                text: root.snapshot.swapMode === "swap-exact-output"
                    ? qsTr("You receive")
                    : qsTr("You receive at least")
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
            }

            Text {
                objectName: "swapReceiveAmount"
                Layout.fillWidth: true
                text: qsTr("%1 %2")
                    .arg(root.snapshot.swapMode === "swap-exact-output"
                        ? root.snapshot.buyAmount || ""
                        : root.snapshot.minReceived || "")
                    .arg(root.snapshot.buyToken || "")
                color: root.theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 18
                elide: Text.ElideRight
            }
        }
    }

    SwapSummary {
        Layout.fillWidth: true
        theme: root.theme
        swapModeText: root.snapshot.swapModeText || ""
        feeText: root.snapshot.feeAmount || ""
        priceImpactText: root.snapshot.priceImpactPercent || ""
        priceImpactPercent: Number(root.snapshot.priceImpactPercentValue) || 0
        slippageText: root.snapshot.slippageTolerance || ""
        limitLabel: root.snapshot.swapMode === "swap-exact-output"
            ? qsTr("Max sent")
            : qsTr("Min received")
        limitText: root.snapshot.swapMode === "swap-exact-output"
            ? qsTr("%1 %2").arg(root.snapshot.maxSent || "").arg(root.snapshot.sellToken || "")
            : qsTr("%1 %2").arg(root.snapshot.minReceived || "").arg(root.snapshot.buyToken || "")
    }
}
