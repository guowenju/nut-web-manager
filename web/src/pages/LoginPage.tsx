import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowRight, LoaderCircle, Network } from 'lucide-react'
import { useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { Navigate, useLocation, useNavigate } from 'react-router'
import { z } from 'zod'
import { version } from '../../package.json'
import { errorMessage, getSession, login, sessionQueryKey } from '../lib/api.ts'

const loginSchema = z.object({
  username: z.string().trim().min(1, '请输入用户名'),
  password: z.string().min(1, '请输入密码'),
})

type LoginForm = z.infer<typeof loginSchema>

export function LoginPage() {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const session = useQuery({
    queryKey: sessionQueryKey,
    queryFn: getSession,
    retry: false,
  })
  const {
    register,
    handleSubmit,
    setFocus,
    formState: { errors },
  } = useForm<LoginForm>({
    resolver: zodResolver(loginSchema),
    defaultValues: { username: 'admin', password: '' },
  })
  const loginMutation = useMutation({
    mutationFn: login,
    onSuccess: (authenticated) => {
      queryClient.setQueryData(sessionQueryKey, authenticated)
      const from = (location.state as { from?: string } | null)?.from ?? '/'
      navigate(from, { replace: true })
    },
  })

  useEffect(() => setFocus('password'), [setFocus])

  if (session.data?.authenticated) return <Navigate to="/" replace />

  return (
    <main className="relative grid min-h-screen place-items-center bg-slate-50 px-5 py-20 text-slate-900">
      <a
        href="https://github.com/guowenju/nut-web-manager"
        target="_blank"
        rel="noopener noreferrer"
        aria-label="查看 GitHub 项目仓库（在新标签页打开）"
        title="GitHub"
        className="absolute right-5 top-5 grid size-10 place-items-center rounded-xl text-slate-500 transition hover:bg-slate-100 hover:text-slate-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50"
      >
        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <path d="M12 .297C5.37.297 0 5.67 0 12.297c0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.043-1.61-4.043-1.61-.546-1.387-1.333-1.756-1.333-1.756-1.09-.745.083-.729.083-.729 1.205.084 1.838 1.237 1.838 1.237 1.07 1.835 2.809 1.305 3.495.998.108-.776.418-1.305.762-1.605-2.665-.3-5.467-1.334-5.467-5.93 0-1.31.467-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23a11.52 11.52 0 0 1 3-.404c1.02.005 2.045.138 3 .404 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
        </svg>
      </a>
      <section className="w-full max-w-sm rounded-2xl border border-slate-200 bg-white p-7 shadow-sm">
        <div className="mb-7 flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl bg-emerald-50 text-emerald-600">
            <Network size={22} />
          </div>
          <p className="font-semibold">NUT Web Manager</p>
        </div>

        <form className="space-y-4" onSubmit={handleSubmit((value) => loginMutation.mutate(value))}>
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-slate-700">用户名</span>
              <input
                {...register('username')}
                autoComplete="username"
                className={loginInputClass}
              />
              {errors.username && <span className="mt-1 block text-xs text-rose-600">{errors.username.message}</span>}
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-slate-700">密码</span>
              <input
                {...register('password')}
                type="password"
                autoComplete="current-password"
                className={loginInputClass}
              />
              {errors.password && <span className="mt-1 block text-xs text-rose-600">{errors.password.message}</span>}
            </label>

            {loginMutation.isError && (
              <div className="rounded-xl border border-rose-200 bg-rose-50 px-3.5 py-3 text-sm text-rose-700">
                {errorMessage(loginMutation.error)}
              </div>
            )}

            <button
              type="submit"
              disabled={loginMutation.isPending}
              className="group mt-2 flex w-full items-center justify-center gap-2 rounded-xl bg-emerald-300 px-4 py-3 text-sm font-semibold text-emerald-950 transition hover:bg-emerald-200 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {loginMutation.isPending ? (
                <LoaderCircle size={17} className="animate-spin" />
              ) : (
                <ArrowRight size={17} className="transition-transform group-hover:translate-x-0.5" />
              )}
              登录
            </button>
        </form>
      </section>
      <p className="absolute bottom-5 right-5 text-xs text-slate-500" aria-label={`应用版本 v${version}`}>
        v{version}
      </p>
    </main>
  )
}

const loginInputClass =
  'w-full rounded-xl border border-slate-200 bg-white px-3.5 py-3 text-sm text-slate-900 outline-none transition placeholder:text-slate-600 focus:border-emerald-500 focus:ring-2 focus:ring-emerald-100'
