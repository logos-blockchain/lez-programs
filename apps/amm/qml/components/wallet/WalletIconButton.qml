import QtQuick
import QtQuick.Controls

import Logos.Theme

// Small icon-only action button for the wallet menu. Uses the same Button +
// icon.source/icon.color path as LogosCopyButton, which renders reliably here
// (LogosIconButton's Image + MultiEffect shader path does not).
Button {
    id: root

    property url iconSource
    property color iconColor: Theme.palette.textSecondary
    property int iconSize: 18

    implicitWidth: 32
    implicitHeight: 32
    display: AbstractButton.IconOnly
    flat: true

    icon.source: root.iconSource
    icon.width: root.iconSize
    icon.height: root.iconSize
    icon.color: root.iconColor
}
