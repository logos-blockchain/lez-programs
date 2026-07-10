import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

FocusScope {
    id: root

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
        color: "#1D1D1D"
        implicitHeight: dialogContent.implicitHeight + 24
        radius: 8
        width: Math.max(0, Math.min(420, root.width - 32))
        border.color: "#343434"
        border.width: 1

        ColumnLayout {
            id: dialogContent

            anchors.fill: parent
            anchors.margins: 12
            spacing: 12

            Text {
                color: "#E7E1D8"
                font.bold: true
                font.pixelSize: 16
                text: qsTr("Confirm new position")

                Layout.fillWidth: true
            }

            Rectangle {
                color: "#151515"
                radius: 8
                border.color: "#343434"
                border.width: 1

                Layout.fillWidth: true
                Layout.preferredHeight: summaryLayout.implicitHeight + 20

                ColumnLayout {
                    id: summaryLayout

                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 8

                    SummaryRow {
                        label: qsTr("Pair")
                        value: root.snapshot.pairText || ""

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Instruction")
                        value: root.snapshot.instructionText || ""

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Fee tier")
                        value: root.snapshot.feeLabel || ""

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Deposit %1").arg(root.snapshot.tokenA || "")
                        value: root.snapshot.depositA || ""

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Deposit %1").arg(root.snapshot.tokenB || "")
                        value: root.snapshot.depositB || ""

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Expected LP")
                        value: root.snapshot.expectedLp || ""

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Minimum LP")
                        value: root.snapshot.minimumLp || ""

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Quote hash")
                        value: root.snapshot.shortQuoteHash || ""

                        Layout.fillWidth: true
                    }
                }
            }

            Text {
                color: "#A9A098"
                font.pixelSize: 12
                lineHeight: 1.25
                text: qsTr("Submit will re-quote against current wallet and chain state before dispatch.")
                wrapMode: Text.WordWrap

                Layout.fillWidth: true
            }

            RowLayout {
                spacing: 8

                Layout.fillWidth: true

                Button {
                    id: cancelButton

                    activeFocusOnTab: true
                    focusPolicy: Qt.StrongFocus
                    hoverEnabled: true
                    text: qsTr("Cancel")

                    Accessible.name: cancelButton.text

                    Layout.fillWidth: true
                    Layout.minimumHeight: 44

                    onClicked: root.cancel()

                    contentItem: Text {
                        color: cancelButton.hovered || cancelButton.activeFocus ? "#151515" : "#E7E1D8"
                        elide: Text.ElideRight
                        font.bold: true
                        font.pixelSize: 13
                        horizontalAlignment: Text.AlignHCenter
                        text: cancelButton.text
                        verticalAlignment: Text.AlignVCenter
                    }

                    background: Rectangle {
                        border.color: cancelButton.activeFocus ? "#F26A21" : "#343434"
                        border.width: 1
                        color: cancelButton.pressed ? "#343434" : cancelButton.hovered || cancelButton.activeFocus ? "#E7E1D8" : "#151515"
                        radius: 6
                    }
                }

                Button {
                    id: confirmButton

                    activeFocusOnTab: true
                    focusPolicy: Qt.StrongFocus
                    hoverEnabled: true
                    text: qsTr("Submit")

                    Accessible.name: confirmButton.text

                    Layout.fillWidth: true
                    Layout.minimumHeight: 44

                    onClicked: root.confirm()

                    contentItem: Text {
                        color: "#151515"
                        elide: Text.ElideRight
                        font.bold: true
                        font.pixelSize: 13
                        horizontalAlignment: Text.AlignHCenter
                        text: confirmButton.text
                        verticalAlignment: Text.AlignVCenter
                    }

                    background: Rectangle {
                        border.color: "#F26A21"
                        border.width: 1
                        color: confirmButton.pressed ? "#D95C1E" : confirmButton.hovered || confirmButton.activeFocus ? "#FF8A3D" : "#F26A21"
                        radius: 6
                    }
                }
            }
        }
    }
}
