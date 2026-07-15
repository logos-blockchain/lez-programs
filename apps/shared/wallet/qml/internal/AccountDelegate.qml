import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ItemDelegate {
    id: root

    required property int index
    required property string name
    required property string address
    required property string balance
    required property bool isPublic

    signal copyRequested(string text)

    leftPadding: 12
    rightPadding: 8
    topPadding: 10
    bottomPadding: 10

    Accessible.name: qsTr("%1, balance %2").arg(root.name).arg(root.balance || "0")

    background: Rectangle {
        color: root.highlighted || root.hovered ? "#27272a" : "#18181b"
        radius: 6
        border.width: root.activeFocus ? 1 : 0
        border.color: "#f26a21"
    }

    contentItem: ColumnLayout {
        spacing: 6

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Label {
                text: root.name
                color: "#f4f4f5"
                font.bold: true
            }

            Label {
                text: root.isPublic ? qsTr("Public") : qsTr("Private")
                color: "#a1a1aa"
                font.pixelSize: 11
            }

            Item { Layout.fillWidth: true }

            Label {
                text: root.balance.length > 0 ? root.balance : "-"
                color: "#f4f4f5"
                font.bold: true
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 4

            Label {
                Layout.fillWidth: true
                text: root.address
                color: "#a1a1aa"
                font.family: "monospace"
                font.pixelSize: 11
                elide: Text.ElideMiddle
            }

            CopyButton {
                visible: root.address.length > 0
                onCopyRequested: root.copyRequested(root.address)
            }
        }
    }
}
