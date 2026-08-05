import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root

    property var theme
    property var snapshot: ({})

    // Exact output guarantees the received amount and caps the spent amount;
    // exact input is the reverse. The wording and which value is the bound flip
    // accordingly.
    readonly property bool isExactOut: (root.snapshot.swapMode || "") === "swap-exact-output"

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
                Layout.fillWidth: true
                text: root.isExactOut ? qsTr("You pay at most") : qsTr("You pay")
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
            }

            Text {
                Layout.fillWidth: true
                text: qsTr("%1 %2")
                    .arg((root.isExactOut ? root.snapshot.boundValue : root.snapshot.sellAmount) || "")
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
                Layout.fillWidth: true
                text: root.isExactOut ? qsTr("You receive exactly") : qsTr("You receive at least")
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
            }

            Text {
                Layout.fillWidth: true
                text: qsTr("%1 %2")
                    .arg((root.isExactOut ? root.snapshot.buyAmount : root.snapshot.boundValue) || "")
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
        boundLabel: root.isExactOut ? qsTr("Maximum sent") : qsTr("Min received")
        boundText: qsTr("%1 %2")
            .arg(root.snapshot.boundValue || "")
            .arg((root.isExactOut ? root.snapshot.sellToken : root.snapshot.buyToken) || "")
    }
}
