import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Popup {
    id: root

    property string title: qsTr("Wallet error")
    property string message: ""

    modal: true
    dim: true
    parent: Overlay.overlay
    width: Math.max(0, Math.min(380, parent ? parent.width - 32 : 380))
    x: parent ? Math.max(0, Math.round((parent.width - width) / 2)) : 0
    y: parent ? Math.max(0, Math.round((parent.height - height) / 2)) : 0
    padding: 20
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    background: Rectangle {
        color: "#18181b"
        border.color: "#3f3f46"
        border.width: 1
        radius: 8
    }

    contentItem: ColumnLayout {
        spacing: 16

        Label {
            Layout.fillWidth: true
            text: root.title
            color: "#f4f4f5"
            font.bold: true
            font.pixelSize: 17
            wrapMode: Text.WordWrap
        }

        Label {
            Layout.fillWidth: true
            text: root.message
            color: "#d4d4d8"
            wrapMode: Text.WordWrap
        }

        Button {
            Layout.alignment: Qt.AlignRight
            text: qsTr("Close")
            onClicked: root.close()
        }
    }
}
