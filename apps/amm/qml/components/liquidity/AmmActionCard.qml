import QtQuick

Rectangle {
    required property var theme

    implicitWidth: 480
    radius: 24
    color: theme.colors.cardBg
    border.color: theme.colors.border
    border.width: 1

    Behavior on color {
        ColorAnimation { duration: 180 }
    }
}
