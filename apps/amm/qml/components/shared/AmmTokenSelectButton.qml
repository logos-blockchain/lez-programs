pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Button {
    id: root

    required property var theme
    property bool hasToken: false
    property string tokenColor: root.theme.colors.noTokenCircle
    property string tokenLetter: ""
    property bool showIndicator: true
    property bool invalid: false
    property real maximumTextWidth: 112

    implicitWidth: contentRow.implicitWidth + 24
    implicitHeight: 40
    leftPadding: 12
    rightPadding: 12
    hoverEnabled: true

    contentItem: RowLayout {
        id: contentRow

        spacing: 6

        Rectangle {
            Layout.preferredWidth: 24
            Layout.preferredHeight: 24
            visible: root.hasToken
            radius: 12
            color: root.tokenColor

            Text {
                anchors.centerIn: parent
                text: root.tokenLetter
                color: "#FFFFFF"
                font.pixelSize: 10
                font.weight: Font.Bold
            }
        }

        Text {
            Layout.maximumWidth: root.maximumTextWidth
            text: root.text
            color: root.enabled
                   ? root.theme.colors.textPrimary
                   : root.theme.colors.textPlaceholder
            font.pixelSize: 15
            font.weight: root.hasToken ? Font.Medium : Font.Normal
            elide: Text.ElideRight
        }

        Text {
            visible: root.showIndicator
            text: "\u25BE"
            color: root.enabled
                   ? root.theme.colors.textSecondary
                   : root.theme.colors.textPlaceholder
            font.pixelSize: 10
        }
    }

    background: Rectangle {
        radius: 20
        color: root.pressed
               ? root.theme.colors.selection
               : root.hovered || root.activeFocus
                 ? root.theme.colors.panelHoverBg
                 : root.theme.colors.panelBg
        border.width: root.invalid || root.activeFocus ? 1 : 0
        border.color: root.invalid ? root.theme.colors.error : root.theme.colors.ctaBg

        Behavior on color {
            ColorAnimation { duration: 120 }
        }
    }
}
