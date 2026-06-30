import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property string message: ""
    property string detail: ""
    property string transactionId: ""
    property string status: "success"
    property bool copied: false
    property bool open: false
    property int duration: 7000

    height: implicitHeight
    implicitHeight: toast.implicitHeight
    opacity: root.open ? 1 : 0
    visible: root.open || fadeOut.running
    z: 30

    TextEdit { id: clipboardProxy; visible: false }

    function shortTransactionId(value) {
        return value.length > 18 ? value.substring(0, 10) + "..." + value.slice(-8) : value;
    }

    function copyTransactionId() {
        if (root.transactionId.length === 0) return;
        clipboardProxy.text = root.transactionId;
        clipboardProxy.selectAll();
        clipboardProxy.copy();
        clipboardProxy.deselect();
        clipboardProxy.text = "";
        root.copied = true;
    }

    function show(nextMessage, nextDetail, nextTransactionId, nextStatus) {
        root.message = nextMessage;
        root.detail = nextDetail || "";
        root.transactionId = nextTransactionId || "";
        root.status = nextStatus || "success";
        root.copied = false;
        root.open = true;
        dismissTimer.restart();
    }

    Timer {
        id: dismissTimer

        interval: root.duration
        repeat: false

        onTriggered: {
            if (copyButton.activeFocus || hoverGuard.containsMouse)
                restart()
            else
                root.open = false
        }
    }

    Behavior on opacity {
        NumberAnimation {
            id: fadeOut

            duration: 160
            easing.type: Easing.OutCubic
        }
    }

    Rectangle {
        id: toast

        anchors.fill: parent
        color: "#20201F"
        implicitHeight: Math.max(50, toastContent.implicitHeight + 18)
        radius: 8
        border.color: root.status === "error" ? "#6A2E2E" : "#4D3A2E"
        border.width: 1
        Accessible.role: Accessible.AlertMessage
        Accessible.name: root.detail.length > 0
                         ? qsTr("%1. %2").arg(root.message).arg(root.detail)
                         : root.message

        MouseArea {
            id: hoverGuard
            anchors.fill: parent
            acceptedButtons: Qt.NoButton
            hoverEnabled: true
        }

        RowLayout {
            id: toastContent

            spacing: 8

            anchors {
                fill: parent
                leftMargin: 14
                rightMargin: 14
            }

            Rectangle {
                color: root.status === "error" ? "#D75C5C" : "#78C88D"
                radius: 6

                Layout.alignment: Qt.AlignTop
                Layout.topMargin: 3
                Layout.preferredHeight: 12
                Layout.preferredWidth: 12
            }

            ColumnLayout {
                spacing: 2

                Layout.fillWidth: true

                Text {
                    id: toastText

                    color: "#E7E1D8"
                    elide: Text.ElideRight
                    font.bold: true
                    font.pixelSize: 14
                    text: root.message

                    Layout.fillWidth: true
                }

                Text {
                    color: "#B8ADA3"
                    font.pixelSize: 12
                    maximumLineCount: 3
                    text: root.detail
                    visible: root.detail.length > 0
                    wrapMode: Text.WordWrap

                    Layout.fillWidth: true
                }

                Text {
                    color: "#F2D8C7"
                    elide: Text.ElideMiddle
                    font.pixelSize: 12
                    text: qsTr("Tx %1").arg(root.shortTransactionId(root.transactionId))
                    visible: root.transactionId.length > 0

                    Layout.fillWidth: true
                }
            }

            Button {
                id: copyButton

                Accessible.name: root.copied ? qsTr("Transaction id copied") : qsTr("Copy transaction id")
                Accessible.role: Accessible.Button
                activeFocusOnTab: true
                focusPolicy: Qt.StrongFocus
                text: root.copied ? qsTr("Copied") : qsTr("Copy tx")
                visible: root.transactionId.length > 0

                Layout.alignment: Qt.AlignVCenter
                Layout.preferredHeight: 30
                Layout.preferredWidth: copyText.implicitWidth + 18

                contentItem: Text {
                    id: copyText

                    color: "#E7E1D8"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: copyButton.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: copyButton.activeFocus ? "#F2D8C7" : "transparent"
                    color: copyButton.hovered ? "#F26A21" : "#2B2724"
                    radius: 6
                }

                onClicked: root.copyTransactionId()
            }
        }
    }
}
