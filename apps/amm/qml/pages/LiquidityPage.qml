import QtQuick 2.15
import QtQuick.Layouts 1.15
import "../components/shared"
import "../components/liquidity"
import "../state"

Item {
    id: root

    property int activeLiquidityTab: 0
    property real slippageTolerancePercent: 0.5
    readonly property int pageMargin: 16
    readonly property int preferredCardWidth: 492
    readonly property int pageCardY: pageCard.implicitHeight + root.pageMargin * 2 <= scroll.height ? Math.round((scroll.height - pageCard.implicitHeight) / 2) : root.pageMargin

    width: parent ? parent.width : implicitWidth
    height: parent ? parent.height : implicitHeight
    implicitWidth: root.preferredCardWidth + root.pageMargin * 2
    implicitHeight: pageCard.implicitHeight + root.pageMargin * 2

    DummyPoolState {
        id: poolState
    }

    Rectangle {
        anchors.fill: parent
        color: "#151515"
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentHeight: Math.max(height, pageCard.y + pageCard.implicitHeight + root.pageMargin)
        contentWidth: width
        enabled: !confirmationDialog.visible
        flickableDirection: Flickable.VerticalFlick

        Rectangle {
            id: pageCard

            color: "#1B1B1B"
            implicitHeight: shellContent.implicitHeight + 24
            radius: 16
            border.color: "#303030"
            border.width: 1
            width: Math.max(0, Math.min(scroll.width - root.pageMargin * 2, root.preferredCardWidth))
            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.pageCardY

            ColumnLayout {
                id: shellContent

                anchors.fill: parent
                anchors.margins: 12
                spacing: 10

                RowLayout {
                    spacing: 10

                    Layout.fillWidth: true

                    Text {
                        color: "#E7E1D8"
                        font.bold: true
                        font.pixelSize: 18
                        text: qsTr("Liquidity")

                        Layout.fillWidth: true
                    }

                    Rectangle {
                        color: "#211914"
                        radius: 12
                        border.color: "#49301F"
                        border.width: 1

                        Layout.preferredHeight: 26
                        Layout.preferredWidth: pairText.implicitWidth + 20

                        Text {
                            id: pairText

                            anchors.centerIn: parent
                            color: "#F2D8C7"
                            font.bold: true
                            font.pixelSize: 12
                            text: qsTr("%1 / %2").arg(poolState.tokenA).arg(poolState.tokenB)
                        }
                    }
                }

                LiquidityActionTabs {
                    currentIndex: root.activeLiquidityTab

                    Layout.fillWidth: true
                    Layout.preferredHeight: implicitHeight

                    onTabRequested: function (index) {
                        root.activeLiquidityTab = index;
                    }
                }

                PoolPositionSummary {
                    poolState: poolState

                    Layout.fillWidth: true
                    Layout.preferredHeight: implicitHeight
                }

                AddLiquidityForm {
                    id: addLiquidityForm

                    poolState: poolState
                    slippageTolerancePercent: root.slippageTolerancePercent
                    visible: root.activeLiquidityTab === 0

                    Layout.fillWidth: true
                    Layout.preferredHeight: visible ? implicitHeight : 0

                    onSlippageToleranceChangeRequested: function (tolerancePercent) {
                        root.slippageTolerancePercent = poolState.clampSlippageTolerancePercent(tolerancePercent);
                    }

                    onAddLiquidityRequested: function (snapshot) {
                        confirmationDialog.openWithSnapshot(snapshot);
                    }
                }

                RemoveLiquidityForm {
                    id: removeLiquidityForm

                    poolState: poolState
                    slippageTolerancePercent: root.slippageTolerancePercent
                    visible: root.activeLiquidityTab === 1

                    Layout.fillWidth: true
                    Layout.preferredHeight: visible ? implicitHeight : 0

                    onSlippageToleranceChangeRequested: function (tolerancePercent) {
                        root.slippageTolerancePercent = poolState.clampSlippageTolerancePercent(tolerancePercent);
                    }

                    onRemoveLiquidityRequested: function (snapshot) {
                        confirmationDialog.openWithSnapshot(snapshot);
                    }
                }
            }

            SuccessToast {
                id: successToast

                width: Math.max(0, Math.min(380, parent.width - 24))

                anchors {
                    bottom: parent.bottom
                    bottomMargin: 14
                    horizontalCenter: parent.horizontalCenter
                }
            }
        }
    }

    LiquidityConfirmationDialog {
        id: confirmationDialog

        anchors.fill: parent

        onConfirmed: function (snapshot) {
            root.confirmLiquidityAction(snapshot);
        }
    }

    function confirmLiquidityAction(snapshot) {
        if (snapshot.action === "add") {
            poolState.applyAddLiquidity(snapshot.actualA, snapshot.actualB, snapshot.deltaLp);
            addLiquidityForm.resetForm();
            successToast.show(qsTr("Liquidity added"), qsTr("Position updated"));
            return;
        }

        if (snapshot.action === "remove") {
            poolState.applyRemoveLiquidity(snapshot.withdrawA, snapshot.withdrawB, snapshot.burnAmount);
            removeLiquidityForm.resetForm();
            successToast.show(qsTr("Liquidity removed"), qsTr("Position updated"));
        }
    }
}
