import { expect, test, type Page } from '@playwright/test'

const PASSWORD = 'e2e-password'

async function login(page: Page) {
  await page.goto('/')
  await expect(page).toHaveURL(/\/login$/)
  await page.getByLabel('パスワード').fill(PASSWORD)
  await page.getByRole('button', { name: 'ログイン' }).click()
  await expect(page.getByRole('navigation', { name: 'フォルダ' })).toBeVisible()
}

async function save(page: Page, ...urls: string[]) {
  const field = page.getByLabel('保存するURL')
  await field.fill(urls.join('\n'))
  await field.press('Enter')
}

const tilesOf = (page: Page, postId: number) => page.locator(`a[data-testid="tile"][href^="/posts/${postId}/"]`)

async function savedPostId(page: Page, statusId: string): Promise<number> {
  const res = await page.request.post('/api/posts', { data: { urls: [`https://x.com/alice/status/${statusId}`] } })
  const [result] = await res.json()
  return result.post_id
}

test('a wrong password is rejected and the right one opens the gallery', async ({ page }) => {
  await page.goto('/')
  await expect(page).toHaveURL(/\/login$/)
  await page.getByLabel('パスワード').fill('not-it')
  await page.getByRole('button', { name: 'ログイン' }).click()
  await expect(page.getByRole('alert')).toContainText('パスワードが違います')

  await page.getByLabel('パスワード').fill(PASSWORD)
  await page.getByRole('button', { name: 'ログイン' }).click()
  await expect(page).toHaveURL('/')
  await expect(page.getByRole('img', { name: 'ARCMAN' }).first()).toBeVisible()
})

test('saving several URLs reports each line and shows one tile per image', async ({ page }) => {
  await login(page)
  await save(page, 'https://x.com/alice/status/1001', 'https://example.com/not-a-post')

  const toast = page.getByRole('status')
  await expect(toast).toContainText('1件を保存しました')
  await expect(toast).toContainText('x.com の投稿URLではありません')
  await expect(toast.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '100')

  await expect(page.getByRole('link', { name: '投稿を開く (1/2)' }).first()).toBeVisible()
  await expect(page.getByRole('link', { name: '投稿を開く (2/2)' }).first()).toBeVisible()

  // A toast with an error stays until closed; one that went through closes itself.
  await toast.getByRole('button', { name: '閉じる' }).click()
  await expect(toast).toBeHidden()
  await save(page, 'https://fixupx.com/alice/status/1001')
  await expect(toast).toContainText('すでに保存されています')
  await expect(toast).toBeHidden({ timeout: 8000 })
})

test('the detail view steps through images by key, keeps a URL per image, and closes with Escape', async ({ page }) => {
  await login(page)
  const id = await savedPostId(page, '1006')
  await page.reload()
  await expect(tilesOf(page, id)).toHaveCount(3)

  await tilesOf(page, id).first().click()
  await expect(page.getByTestId('counter')).toHaveText('1 / 3')
  await page.keyboard.press('ArrowRight')
  await expect(page.getByTestId('counter')).toHaveText('2 / 3')
  await expect(page).toHaveURL(new RegExp(`/posts/${id}/1$`))

  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toBeHidden()
  await expect(page).toHaveURL('/')
})

test('a text post shows its translation, the missing quote and the link card', async ({ page }) => {
  await login(page)
  const id = await savedPostId(page, '1003')
  await page.goto(`/posts/${id}/0`)

  const panel = page.getByRole('complementary', { name: '投稿の内容' })
  await expect(panel).toContainText('港に差す朝の光。')
  await expect(panel).toContainText('引用元は表示できません')
  await expect(panel).toContainText('港町の朝を歩く')
  await expect(panel).toContainText('いいね')
})

test('video tiles show the duration', async ({ page }) => {
  await login(page)
  const id = await savedPostId(page, '1008')
  await page.reload()
  await expect(tilesOf(page, id)).toContainText('0:25')
})

test('the tile size steps up and down and is kept across reloads', async ({ page }) => {
  await login(page)
  const id = await savedPostId(page, '1006')
  await page.reload()
  await expect(tilesOf(page, id)).toHaveCount(3)
  const lastRowHeight = () => page.locator('.gallery-row').last().evaluate((el) => el.getBoundingClientRect().height)

  const before = await lastRowHeight()
  await page.getByRole('button', { name: 'タイルを大きく' }).click()
  await expect.poll(lastRowHeight).toBeGreaterThan(before)
  const larger = await lastRowHeight()

  await page.reload()
  await expect.poll(lastRowHeight).toBe(larger)
  await page.getByRole('button', { name: 'タイルを小さく' }).click()
  await page.getByRole('button', { name: 'タイルを小さく' }).click()
  await expect.poll(lastRowHeight).toBeLessThan(before)
})

test('folders can be created in a category, receive saved posts, and take their posts when deleted', async ({ page }) => {
  await login(page)
  const nav = page.getByRole('navigation', { name: 'フォルダ' })

  await nav.getByRole('button', { name: 'カテゴリを追加' }).click()
  await nav.getByLabel('新しいカテゴリの名前').fill('趣味')
  await nav.getByLabel('新しいカテゴリの名前').press('Enter')
  await nav.getByRole('button', { name: '「趣味」にフォルダを追加' }).click()
  await nav.getByLabel('新しいフォルダの名前').fill('旅行')
  await nav.getByLabel('新しいフォルダの名前').press('Enter')

  await nav.getByRole('link', { name: '旅行' }).click()
  await expect(page.getByRole('heading', { name: '旅行' })).toBeVisible()
  await save(page, 'https://x.com/alice/status/1004')
  await expect(page.getByRole('status')).toContainText('1件を保存しました')
  await expect(page.locator('a[data-testid="tile"]')).toHaveCount(1)
  await expect(page.locator('a[data-testid="tile"].photo')).toHaveCount(1)
  const href = await page.locator('a[data-testid="tile"]').getAttribute('href')
  const postId = Number(href!.split('/')[2])

  await nav.getByRole('button', { name: 'その他の操作: 旅行' }).click()
  await nav.getByRole('menuitem', { name: '削除' }).click()
  const dialog = page.getByRole('alertdialog')
  await expect(dialog).toContainText('1 件の投稿')
  await dialog.getByRole('button', { name: '1件と一緒に削除' }).click()

  await expect(page).toHaveURL('/')
  await expect(nav.getByRole('link', { name: '旅行' })).toHaveCount(0)
  await expect(tilesOf(page, postId)).toHaveCount(0)
})

test('tags are added in the detail panel, filter the gallery, and can be renamed', async ({ page }) => {
  await login(page)
  const id = await savedPostId(page, '1005')
  await page.goto(`/posts/${id}/0`)

  const tagField = page.getByRole('combobox', { name: 'タグを追加' })
  await tagField.fill('Cats')
  await tagField.press('Enter')
  await expect(page.getByRole('button', { name: 'タグ「Cats」を外す' })).toBeVisible()
  await page.getByRole('button', { name: '閉じる' }).click()

  await page.getByRole('button', { name: 'タグ', exact: true }).click()
  await page.getByRole('checkbox', { name: /Cats/ }).click()
  await expect(page.getByRole('checkbox', { name: /Cats/ })).toBeChecked()
  await expect(page).toHaveURL(/tags=\d+/)
  await expect(page.locator('a[data-testid="tile"]')).toHaveCount(1)
  await expect(tilesOf(page, id)).toHaveCount(1)

  await page.goto('/tags')
  await page.getByRole('button', { name: '「Cats」の名前を変える' }).click()
  await page.getByLabel('新しいタグ名').fill('Kittens')
  await page.getByRole('button', { name: '保存' }).click()
  await expect(page.getByRole('row', { name: /Kittens/ })).toBeVisible()
})

test.describe('on a phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true })

  test('the drawer switches folders and a swipe moves to the next image', async ({ page }) => {
    await login(page)
    const id = await savedPostId(page, '1007')

    await page.getByRole('button', { name: 'フォルダを開く' }).click()
    const nav = page.getByRole('navigation', { name: 'フォルダ' })
    await nav.getByRole('link', { name: '未分類' }).click()
    await expect(page.getByRole('heading', { name: '未分類' })).toBeVisible()

    await expect(tilesOf(page, id)).toHaveCount(2)
    await tilesOf(page, id).first().click()
    await expect(page.getByTestId('counter')).toHaveText('1 / 2')

    const stage = page.getByRole('region', { name: 'メディア' })
    const box = (await stage.boundingBox())!
    await page.mouse.move(box.x + box.width * 0.8, box.y + box.height * 0.4)
    await page.mouse.down()
    await page.mouse.move(box.x + box.width * 0.2, box.y + box.height * 0.4, { steps: 5 })
    await page.mouse.up()
    await expect(page.getByTestId('counter')).toHaveText('2 / 2')
  })
})
