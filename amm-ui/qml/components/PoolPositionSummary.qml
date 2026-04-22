import QtQuick 2.15
import QtQuick.Layouts 1.15
import "../state"

Rectangle {
    id: root

    required property DummyPoolState poolState
    readonly property string estimateHelp: qsTr("This value is an estimate from the current dummy reserves and your share of total LP supply.")

    color: "#1D1D1D"
    implicitHeight: content.implicitHeight + 20
    radius: 8
    border.color: "#343434"
    border.width: 1

    ColumnLayout {
        id: content

        anchors.fill: parent
        anchors.margins: 10
        spacing: 4

        Text {
            color: "#E7E1D8"
            font.bold: true
            font.pixelSize: 16
            text: qsTr("Pool position")

            Layout.fillWidth: true
        }

        Text {
            color: "#F26A21"
            font.pixelSize: 12
            text: qsTr("You have no position in this pool")
            visible: root.poolState.userLpBalance === 0

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Token A")
            value: root.poolState.tokenA

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Token B")
            value: root.poolState.tokenB

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Fee tier")
            value: root.poolState.feeTier

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Your LP tokens")
            value: root.poolState.formatInteger(root.poolState.userLpBalance)
            visible: root.poolState.userLpBalance > 0

            Layout.fillWidth: true
        }

        SummaryRow {
            estimated: true
            estimateHelp: root.estimateHelp
            label: qsTr("Pool share")
            value: root.poolState.formatPoolShare(root.poolState.poolShare)
            visible: root.poolState.userLpBalance > 0

            Layout.fillWidth: true
        }

        SummaryRow {
            estimated: true
            estimateHelp: root.estimateHelp
            label: qsTr("Your Token A")
            value: "\u2248 " + root.poolState.formatTokenAmount(root.poolState.userOwnedA, root.poolState.tokenA)
            visible: root.poolState.userLpBalance > 0

            Layout.fillWidth: true
        }

        SummaryRow {
            estimated: true
            estimateHelp: root.estimateHelp
            label: qsTr("Your Token B")
            value: "\u2248 " + root.poolState.formatTokenAmount(root.poolState.userOwnedB, root.poolState.tokenB)
            visible: root.poolState.userLpBalance > 0

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Total reserve A")
            value: root.poolState.formatTokenAmount(root.poolState.reserveA, root.poolState.tokenA)

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Total reserve B")
            value: root.poolState.formatTokenAmount(root.poolState.reserveB, root.poolState.tokenB)

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Total LP supply")
            value: root.poolState.formatInteger(root.poolState.totalLpSupply)

            Layout.fillWidth: true
        }
    }
}
