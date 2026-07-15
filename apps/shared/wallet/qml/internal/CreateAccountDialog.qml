import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Popup {
    id: root

    property bool busy: false

    signal createRequested(bool isPublic)

    modal: true
    dim: true
    parent: Overlay.overlay
    width: Math.max(0, Math.min(380, parent ? parent.width - 32 : 380))
    x: parent ? Math.max(0, Math.round((parent.width - width) / 2)) : 0
    y: parent ? Math.max(0, Math.round((parent.height - height) / 2)) : 0
    padding: 20
    closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape | Popup.CloseOnPressOutside

    onOpened: privateSwitch.checked = false

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
            text: qsTr("Create account")
            color: "#f4f4f5"
            font.bold: true
            font.pixelSize: 17
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                Label {
                    text: privateSwitch.checked ? qsTr("Private") : qsTr("Public")
                    color: "#f4f4f5"
                    font.bold: true
                }

                Label {
                    Layout.fillWidth: true
                    text: privateSwitch.checked
                        ? qsTr("Private balance and activity")
                        : qsTr("Public balance and activity")
                    color: "#a1a1aa"
                    wrapMode: Text.WordWrap
                }
            }

            Switch {
                id: privateSwitch
                objectName: "privateAccountSwitch"
                Accessible.name: qsTr("Create private account")
                enabled: !root.busy
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10

            Button {
                Layout.fillWidth: true
                text: qsTr("Cancel")
                enabled: !root.busy
                onClicked: root.close()
            }

            Button {
                id: createButton
                objectName: "createAccountButton"
                Layout.fillWidth: true
                text: root.busy ? qsTr("Creating...") : qsTr("Create")
                enabled: !root.busy
                onClicked: root.createRequested(!privateSwitch.checked)
            }
        }
    }
}
