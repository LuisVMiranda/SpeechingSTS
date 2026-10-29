Option Explicit
Dim shell, files, folder, program
Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
folder = files.GetParentFolderName(WScript.ScriptFullName)
program = files.BuildPath(folder, "speeching.exe")
If Not files.FileExists(program) Then
  MsgBox "speeching.exe is missing. Build it with cargo build --release --locked.", 48, "Speeching"
Else
  shell.Run Chr(34) & program & Chr(34), 1, False
End If
