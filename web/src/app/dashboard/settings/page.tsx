"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardTitle, CardDescription } from "@/components/ui/card";
import { updateProfile, changePassword } from "@/lib/api";
import { useAuth } from "@/lib/auth-context";

export default function SettingsPage() {
  const { user, refresh } = useAuth();
  const [displayName, setDisplayName] = useState("");
  const [email, setEmail] = useState("");
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [profileMsg, setProfileMsg] = useState("");
  const [passwordMsg, setPasswordMsg] = useState("");
  const [saving, setSaving] = useState(false);
  const [changingPw, setChangingPw] = useState(false);

  useEffect(() => {
    if (user) {
      setDisplayName(user.displayName || "");
      setEmail(user.email);
    }
  }, [user]);

  async function handleProfileSave(e: React.FormEvent) {
    e.preventDefault(); setSaving(true); setProfileMsg("");
    try {
      const updates: Record<string, string> = {};
      if (displayName !== (user?.displayName || "")) updates.displayName = displayName;
      if (email !== user?.email) updates.email = email;
      if (Object.keys(updates).length === 0) { setProfileMsg("No changes"); setSaving(false); return; }
      const resp = await updateProfile(updates);
      setProfileMsg(resp.message);
      await refresh();
    } catch (err) { setProfileMsg(err instanceof Error ? err.message : "Update failed"); }
    finally { setSaving(false); }
  }

  async function handlePasswordChange(e: React.FormEvent) {
    e.preventDefault(); setChangingPw(true); setPasswordMsg("");
    try {
      const resp = await changePassword(currentPassword, newPassword);
      setPasswordMsg(resp.message); setCurrentPassword(""); setNewPassword("");
    } catch (err) { setPasswordMsg(err instanceof Error ? err.message : "Failed"); }
    finally { setChangingPw(false); }
  }

  if (!user) return null;

  return (
    <div>
      <div className="mb-8">
        <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Settings</h1>
        <p className="text-[14px] text-muted mt-1">Manage your account</p>
      </div>

      <div className="space-y-5">
        <Card>
          <CardTitle>Profile</CardTitle>
          <CardDescription className="mb-4">Your display name and email</CardDescription>
          <form onSubmit={handleProfileSave} className="space-y-4">
            <Input id="displayName" label="Display Name" value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
            <Input id="email" label="Email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
            {profileMsg && <p className="text-[13px] text-muted">{profileMsg}</p>}
            <Button type="submit" loading={saving}>Save Changes</Button>
          </form>
        </Card>

        <Card>
          <CardTitle>Change Password</CardTitle>
          <CardDescription className="mb-4">Update your account password</CardDescription>
          <form onSubmit={handlePasswordChange} className="space-y-4">
            <Input id="currentPw" label="Current Password" type="password" value={currentPassword} onChange={(e) => setCurrentPassword(e.target.value)} required />
            <Input id="newPw" label="New Password" type="password" value={newPassword} onChange={(e) => setNewPassword(e.target.value)} required minLength={8} />
            {passwordMsg && <p className="text-[13px] text-muted">{passwordMsg}</p>}
            <Button type="submit" variant="secondary" loading={changingPw}>Change Password</Button>
          </form>
        </Card>

        <Card>
          <CardTitle>Account</CardTitle>
          <div className="space-y-2.5 mt-3 text-[13px]">
            <div className="flex justify-between"><span className="text-muted">User ID</span><span className="font-mono text-[12px]">{user.userId}</span></div>
            <div className="flex justify-between"><span className="text-muted">Plan</span><span className="capitalize">{user.plan}</span></div>
            <div className="flex justify-between"><span className="text-muted">Email Verified</span><span>{user.emailVerified ? "✓ Yes" : "✗ No"}</span></div>
            <div className="flex justify-between"><span className="text-muted">Member Since</span><span>{new Date(user.createdAt).toLocaleDateString()}</span></div>
          </div>
        </Card>
      </div>
    </div>
  );
}
