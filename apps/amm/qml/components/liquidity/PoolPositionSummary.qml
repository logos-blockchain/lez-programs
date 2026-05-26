import QtQuick 2.15
import QtQuick.Layouts 1.15
import "../../state"

Rectangle {
    id: root

    required property DummyPoolState poolState
    readonly property string estimateHelp: qsTr("This value is an estimate from the current dummy reserves and your share of total LP supply.")

    color: "#151515"
    implicitHeight: content.implicitHeight + 20
    radius: 8
    border.color: "#303030"
    border.width: 1

    ColumnLayout {
        id: content

        anchors.fill: parent
        anchors.margins: 10
        spacing: 6

        RowLayout {
            spacing: 10

            Layout.fillWidth: true

            ColumnLayout {
                spacing: 2

                Layout.fillWidth: true

                Text {
                    color: "#E7E1D8"
                    font.bold: true
                    font.pixelSize: 13
                    text: root.poolState.userLpBalance > 0 ? qsTr("Your position") : qsTr("No position")

                    Layout.fillWidth: true
                }

                Text {
                    color: "#8E8780"
                    font.pixelSize: 11
                    text: qsTr("%1 LP tokens").arg(root.poolState.formatInteger(root.poolState.userLpBalance))
                    visible: root.poolState.userLpBalance > 0

                    Layout.fillWidth: true
                }
            }

            Rectangle {
                color: "#211914"
                radius: 10
                border.color: "#49301F"
                border.width: 1

                Layout.preferredHeight: 24
                Layout.preferredWidth: shareText.implicitWidth + 18

                Text {
                    id: shareText

                    anchors.centerIn: parent
                    color: "#F2D8C7"
                    font.bold: true
                    font.pixelSize: 11
                    text: root.poolState.userLpBalance > 0 ? root.poolState.formatPoolShare(root.poolState.poolShare) : root.poolState.feeTier
                }
            }
        }

        SummaryRow {
            estimated: true
            estimateHelp: root.estimateHelp
            label: qsTr("Owned")
            value: qsTr("%1 + %2").arg(root.poolState.formatCompactTokenAmount(root.poolState.userOwnedA, root.poolState.tokenA)).arg(root.poolState.formatCompactTokenAmount(root.poolState.userOwnedB, root.poolState.tokenB))
            visible: root.poolState.userLpBalance > 0

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Pool")
            value: qsTr("%1 / %2").arg(root.poolState.formatCompactTokenAmount(root.poolState.reserveA, root.poolState.tokenA)).arg(root.poolState.formatCompactTokenAmount(root.poolState.reserveB, root.poolState.tokenB))

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Fee")
            value: root.poolState.feeTier

            Layout.fillWidth: true
        }
    }
}
