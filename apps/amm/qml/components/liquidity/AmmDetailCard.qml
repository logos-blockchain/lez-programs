import QtQuick
import QtQuick.Layouts

// A titled read-only card: a small section heading over a column of rows.
// Children go straight into the body column, so callers just declare their rows.
Rectangle {
    id: root

    required property var theme
    property string title: ""
    property int contentSpacing: 10
    default property alias content: body.data

    implicitHeight: layout.implicitHeight + 40
    radius: 16
    color: root.theme.colors.cardBg
    border.color: root.theme.colors.border
    border.width: 1

    ColumnLayout {
        id: layout

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.leftMargin: 20
        anchors.rightMargin: 20
        anchors.topMargin: 20
        spacing: 14

        Text {
            Layout.fillWidth: true
            text: root.title
            color: root.theme.colors.textSecondary
            font.pixelSize: 12
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }

        ColumnLayout {
            id: body

            Layout.fillWidth: true
            spacing: root.contentSpacing
        }
    }
}
