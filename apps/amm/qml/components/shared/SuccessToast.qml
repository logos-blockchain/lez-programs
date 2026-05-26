import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property string message: ""
    property string detail: ""
    property bool open: false
    property int duration: 3600

    height: implicitHeight
    implicitHeight: toast.implicitHeight
    opacity: root.open ? 1 : 0
    visible: root.open || fadeOut.running
    z: 30

    function show(nextMessage, nextDetail) {
        root.message = nextMessage;
        root.detail = nextDetail || "";
        root.open = true;
        dismissTimer.restart();
    }

    Timer {
        id: dismissTimer

        interval: root.duration
        repeat: false

        onTriggered: root.open = false
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
        border.color: "#4D3A2E"
        border.width: 1

        RowLayout {
            id: toastContent

            spacing: 8

            anchors {
                fill: parent
                leftMargin: 14
                rightMargin: 14
            }

            Rectangle {
                color: "#78C88D"
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
                    elide: Text.ElideRight
                    font.pixelSize: 12
                    text: root.detail
                    visible: root.detail.length > 0

                    Layout.fillWidth: true
                }
            }
        }
    }
}
