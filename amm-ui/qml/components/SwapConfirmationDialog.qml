import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

FocusScope {
    id: root

    property var theme
    property var snapshot: ({})
    property bool open: false

    signal canceled
    signal confirmed(var snapshot)

    visible: root.open
    z: 20

    Keys.onEscapePressed: root.cancel()

    function openWithSnapshot(nextSnapshot) {
        root.snapshot = nextSnapshot;
        root.open = true;
        root.forceActiveFocus();
        cancelButton.forceActiveFocus();
    }

    function cancel() {
        root.open = false;
        root.canceled();
    }

    function confirm() {
        const confirmedSnapshot = root.snapshot;
        root.open = false;
        root.confirmed(confirmedSnapshot);
    }

    Rectangle {
        anchors.fill: parent
        color: "#99000000"

        MouseArea {
            anchors.fill: parent
        }
    }

    Rectangle {
        id: panel

        anchors.centerIn: parent
        color: root.theme.colors.cardBg
        implicitHeight: dialogContent.implicitHeight + 32
        radius: 16
        width: Math.max(0, Math.min(380, root.width - 32))
        border.color: root.theme.colors.border
        border.width: 1

        MouseArea {
            anchors.fill: parent
        }

        ColumnLayout {
            id: dialogContent

            anchors.fill: parent
            anchors.margins: 16
            spacing: 14

            Text {
                color: root.theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 17
                text: qsTr("Confirm swap")
                Layout.fillWidth: true
            }

            ColumnLayout {
                spacing: 10
                Layout.fillWidth: true

                Rectangle {
                    Layout.fillWidth: true
                    color: root.theme.colors.inputBg
                    radius: 12
                    implicitHeight: payColumn.implicitHeight + 24

                    ColumnLayout {
                        id: payColumn
                        anchors.fill: parent
                        anchors.margins: 12
                        spacing: 4

                        Text {
                            text: qsTr("You pay")
                            color: root.theme.colors.textSecondary
                            font.pixelSize: 12
                            Layout.fillWidth: true
                        }
                        Text {
                            text: qsTr("%1 %2")
                                    .arg(root.snapshot.sellAmount || "")
                                    .arg(root.snapshot.sellToken || "")
                            color: root.theme.colors.textPrimary
                            font.bold: true
                            font.pixelSize: 18
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    color: root.theme.colors.inputBg
                    radius: 12
                    implicitHeight: receiveColumn.implicitHeight + 24

                    ColumnLayout {
                        id: receiveColumn
                        anchors.fill: parent
                        anchors.margins: 12
                        spacing: 4

                        Text {
                            text: qsTr("You receive at least")
                            color: root.theme.colors.textSecondary
                            font.pixelSize: 12
                            Layout.fillWidth: true
                        }
                        Text {
                            text: qsTr("%1 %2")
                                    .arg(root.snapshot.minReceived || "")
                                    .arg(root.snapshot.buyToken || "")
                            color: root.theme.colors.textPrimary
                            font.bold: true
                            font.pixelSize: 18
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
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
                minReceivedText: qsTr("%1 %2")
                                    .arg(root.snapshot.minReceived || "")
                                    .arg(root.snapshot.buyToken || "")
            }

            RowLayout {
                spacing: 10
                Layout.fillWidth: true
                Layout.topMargin: 4

                Button {
                    id: cancelButton
                    activeFocusOnTab: true
                    focusPolicy: Qt.StrongFocus
                    hoverEnabled: true
                    text: qsTr("Cancel")
                    Layout.fillWidth: true
                    Layout.minimumHeight: 48
                    onClicked: root.cancel()

                    contentItem: Text {
                        color: root.theme.colors.textPrimary
                        elide: Text.ElideRight
                        font.bold: true
                        font.pixelSize: 14
                        horizontalAlignment: Text.AlignHCenter
                        text: cancelButton.text
                        verticalAlignment: Text.AlignVCenter
                    }
                    background: Rectangle {
                        border.color: root.theme.colors.borderStrong
                        border.width: 1
                        color: cancelButton.pressed
                            ? root.theme.colors.panelHoverBg
                            : cancelButton.hovered || cancelButton.activeFocus
                                ? root.theme.colors.panelBg
                                : "transparent"
                        radius: 14
                    }
                }

                Button {
                    id: confirmButton
                    activeFocusOnTab: true
                    focusPolicy: Qt.StrongFocus
                    hoverEnabled: true
                    text: qsTr("Confirm Swap")
                    Layout.fillWidth: true
                    Layout.minimumHeight: 48
                    onClicked: root.confirm()

                    contentItem: Text {
                        color: "#ffffff"
                        elide: Text.ElideRight
                        font.bold: true
                        font.pixelSize: 14
                        horizontalAlignment: Text.AlignHCenter
                        text: confirmButton.text
                        verticalAlignment: Text.AlignVCenter
                    }
                    background: Rectangle {
                        border.width: 0
                        color: confirmButton.pressed
                            ? "#D95C1E"
                            : confirmButton.hovered || confirmButton.activeFocus
                                ? root.theme.colors.ctaHoverBg
                                : root.theme.colors.ctaBg
                        radius: 14
                    }
                }
            }
        }
    }
}
